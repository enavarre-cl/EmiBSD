/* $OpenBSD: if_em.c,v 1.384 2026/08/14 06:40:24 jsg Exp $ */
/* $OpenBSD: if_em.h,v 1.84 2026/03/04 14:15:36 bluhm Exp $ */
/* $FreeBSD: if_em.c,v 1.46 2004/09/29 18:28:28 mlaier Exp $ */
/* $FreeBSD: if_em.h,v 1.26 2004/09/01 23:22:41 pdeuskar Exp $ */
/* <LICENSES> */
/**************************************************************************

Copyright (c) 2001-2003, Intel Corporation
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

 1. Redistributions of source code must retain the above copyright notice,
    this list of conditions and the following disclaimer.

 2. Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in the
    documentation and/or other materials provided with the distribution.

 3. Neither the name of the Intel Corporation nor the names of its
    contributors may be used to endorse or promote products derived from
    this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
POSSIBILITY OF SUCH DAMAGE.

***************************************************************************/
/* </LICENSES> */

//! em(4): the driver of Intel's PRO/1000 Gigabit Ethernet controllers (`em* at pci?`), the
//! 8254x, 8257x, 82580, i210/i350, the ICH8 to PCH LAN MACs and their kin
//! (`<dev/pci/if_em.h>` and `if_em.c`).
//!
//! Upstream: sys/dev/pci/if_em.h @ 3ce1f3f79392, sys/dev/pci/if_em.c @ 3ce1f3f79392
//!
//! The MAC, PHY and NVM work is the shared code's (`if_em_hw.rs`); this file is the driver
//! proper: the PCI match against `em_devices[]`, the attach (the register BAR, the I/O and
//! flash BARs where the MAC needs them, one queue, MSI or INTx, or MSI-X when
//! `em_enable_msix` asks for it, the EEPROM checks and the MAC address, the `ifnet`), `init`
//! and `stop`, one transmit and one receive descriptor ring per queue in `bus_dma(9)`
//! memory, the interrupt handlers, the one-second timer (SmartSpeed), the transmit
//! watchdog, the ioctls, the multicast filter, the link state, checksum offload (the legacy
//! context descriptor up to the 8257x, the advanced one and TSO on the 82575 to i210) and
//! the 82547 half-duplex FIFO and 82544 PCI-X workarounds.
//!
//! The softc owns the shared code's `struct em_hw` and `struct em_osdep`; `EmPciOps`, the
//! functions the shared code calls back into (`em_pci_set_mwi`, `em_read_pci_cfg`, ...),
//! are defined here, as in the C, and handed to `EmHw::new`.
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the device first and the `struct arpcom` inside, made of
//!   `Cell`s and atomics (all-zero valid). `osdep`, `hw` and `regs` (below) are
//!   `UnsafeCell<MaybeUninit<..>>`: the attach writes them once, before they are read.
//! - Locking of `struct em_hw`. The C changes `sc->hw` only under the kernel lock (the
//!   ioctls, the timeouts, the attach, and `em_intr`/`em_link_intr_msix`, which take
//!   `KERNEL_LOCK()` around the link check) and at `splnet`, while the MP-safe paths
//!   (`em_start`, `em_encap`, the ring work of the interrupt handlers, `em_txeof`,
//!   `em_rxeof`) only read `mac_type` and reach the registers. Here `em_hw()` hands out the
//!   `&mut EmHw` behind a guard that asserts the kernel lock and panics on a second live
//!   borrow, and the MP-safe paths read `regs`: a second `struct em_hw` with the same `back`
//!   (the register handles) and `mac_type`, written by the attach before the interrupt is
//!   established and never changed again. `TBI_ACCEPT`, which `em_rxeof` evaluates, reads
//!   `tbi_compatibility_on`, `min_frame_size` and `max_frame_size`: these are copied into
//!   atomics of the softc whenever the guard is dropped (`tbi_accept_with` in
//!   `if_em_hw.rs`).
//! - The transmit ring's head and tail are atomics (`em_start` moves the head on whatever
//!   CPU runs the send queue, `em_txeof` the tail in the interrupt); so are `link_active`
//!   and `link_duplex`, which `em_start` reads unlocked, and the 82547 FIFO state, which
//!   `em_start` and the FIFO timeout both change. The receive ring stays `Cell`s: as in C,
//!   only the interrupt and the refill timeout touch it, both at `IPL_NET` on the primary
//!   CPU, where the interrupt is established and the timeouts run.
//! - The descriptor rings are DMA memory the controller writes: they are reached through raw
//!   pointers with bounds-checked accessors, every access a whole-descriptor
//!   `read_volatile`/`write_volatile` (`|=` on a descriptor is a read, a change and a
//!   write, done before the tail register hands the descriptor to the controller); the
//!   context descriptors are built as values and written into their slot.
//! - The osdep is written by `em_allocate_pci_resources` once the register BAR is mapped
//!   (the C fills it member by member, starting from zeroes). Until the I/O and flash BARs
//!   are mapped, their tags and handles are the register BAR's, with zero bases and sizes,
//!   which is what `em_free_pci_resources` tests. `em_identify_hardware` reads the PCI
//!   attach arguments it is given (the C's `&sc->osdep.em_pa`, the same copy of `*pa`).
//!   `hw.back` points at the osdep from the start: what runs on `hw` before the osdep is
//!   written (`em_set_mac_type` and member assignments) does not reach it, as the C runs it
//!   with `hw->back` still NULL.
//! - `em_setup_interface` adds the media words `em_media_words` lists, in the C's order.
//! - `kstat(4)` is not configured (`NKSTAT` 0): `em_kstat_attach`, `em_kstat_read`,
//!   `em_tbi_adjust_stats`, `enum em_stat` and `em_counters[]` are compiled out, as in such
//!   a C kernel, with comments at the call sites; the softc's `kstat` and `kstat_mtx` are
//!   left out.
//! - `vlan(4)` is not configured: the C's `#if NVLAN > 0` blocks are ported behind the
//!   constant [`NVLAN`] (0), so `IFCAP_VLAN_HWTAGGING` is not offered and the tags are
//!   neither inserted nor taken from the descriptors.
//! - `SMALL_KERNEL` is not defined: the MSI-X functions are here; `em_enable_msix` is 0 as
//!   in C (an atomic a debugger may change). The MSI-X queue's interrupt name is a copy
//!   that is never freed (`em_intr_name`), as `evcount(9)` keeps the pointer.
//! - `DEBUG_INIT`, `DEBUG_IOCTL` and `DEBUG_HW` are 0 and `EM_DEBUG`/`EM_MASTER_SLAVE` are
//!   not defined: the `*_DEBUGOUT*` macros (empty) are left out.
//! - Functions returning 0 or an errno return `Result<(), Errno>`; `em_encap` and the
//!   offload setups return the slot counts as `u32`; `em_rxfill`/`em_rxeof` return `bool`.
//!   `em_init_hw` takes the hardware and `num_queues` (`if_em_hw.rs`).
//! - `em_rxeof` reads the frame's last byte (for `TBI_ACCEPT`) only when the descriptor's
//!   length is not 0; the C reads the byte before the buffer then.
//! - `intr_barrier` is skipped when `sc_intrhand` is NULL.
//! - `offsetof(struct tcphdr, th_sum)` and `offsetof(struct udphdr, uh_sum)` are the
//!   constants 16 and 6: `netinet/tcp.h` and `netinet/udp.h` are not ported.

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::mem::{MaybeUninit, offset_of};
use core::ops::{Deref, DerefMut};
use core::ptr::{self, NonNull};
use core::sync::atomic::{
    AtomicBool, AtomicI32, AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering,
};

use crate::dev::pci::if_em_hw::*;
use crate::dev::pci::if_em_osdep::{
    CMD_MEM_WRT_INVALIDATE, EmOsdep, e1000_read_reg, e1000_read_reg_array, e1000_write_flush,
    e1000_write_reg, e1000_write_reg_array, em_read_reg, em_write_reg, msec_delay, usec_delay,
};
use crate::dev::pci::if_em_soc::{em_attach_miibus, em_lookup_gcu};
use crate::dev::pci::pci::{pci_get_capability, pci_matchbyid};
use crate::dev::pci::pci_map::pci_mapreg_map;
use crate::dev::pci::pcidevs::*;
use crate::dev::pci::pcireg::{
    PCI_CAP_PCIEXPRESS, PCI_CLASS_REG, PCI_COMMAND_STATUS_REG, PCI_MAPREG_END,
    PCI_MAPREG_MEM_TYPE_64BIT, PCI_MAPREG_START, PCI_MAPREG_TYPE, PCI_MAPREG_TYPE_IO,
    PCI_MAPREG_TYPE_MEM, PCI_PCIE_LCSR, PCI_PCIE_LCSR_ASPM_L0S, PCI_PCIE_LCSR_ASPM_L1,
    PCI_SUBSYS_ID_REG, PciProductId, PciVendorId, pci_mapreg_mem_type, pci_product, pci_revision,
    pci_vendor,
};
use crate::dev::pci::pcivar::{PCI_FLAGS_MSI_ENABLED, PciAttachArgs, PciMatchid};
use crate::kassert;
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_rwlock::{rw_enter, rw_exit, rw_init};
use crate::kern::kern_timeout::{timeout_add, timeout_add_sec, timeout_del, timeout_set};
use crate::kern::subr_autoconf::config_defer;
use crate::kern::subr_prf::{Str, panic, printf};
use crate::kern::uipc_mbuf::{m_adj, m_clget, m_defrag, m_freem, ml_enqueue};
use crate::machine::bus::{
    BUS_DMA_64BIT, BUS_DMA_ALLOCNOW, BUS_DMA_COHERENT, BUS_DMA_NOWAIT, BUS_DMA_WAITOK,
    BUS_DMA_ZERO, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD,
    BUS_DMASYNC_PREWRITE, BUS_SPACE_BARRIER_READ, BUS_SPACE_BARRIER_WRITE, BusDmaSegment,
    BusDmaTag, BusDmamap, BusSize, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load,
    bus_dmamap_load_mbuf, bus_dmamap_sync, bus_dmamap_unload, bus_dmamem_alloc, bus_dmamem_free,
    bus_dmamem_map, bus_dmamem_unmap, bus_space_barrier, bus_space_unmap,
};
use crate::machine::intr::{IPL_MPSAFE, IPL_NET, intr_barrier, splnet, splx};
use crate::machine::pci_machdep::{
    pci_conf_read, pci_conf_write, pci_intr_disestablish, pci_intr_establish, pci_intr_map,
    pci_intr_map_msi, pci_intr_map_msix, pci_intr_string,
};
use crate::net::bpf::{BPF_DIRECTION_OUT, bpf_mtap_ether};
use crate::net::ethertypes::ETHERTYPE_VLAN;
use crate::net::if_::{
    IFCAP_CSUM_IPv4, IFCAP_CSUM_TCPv4, IFCAP_CSUM_TCPv6, IFCAP_CSUM_UDPv4, IFCAP_CSUM_UDPv6,
    IFCAP_TSOv4, IFCAP_TSOv6, IFCAP_VLAN_HWTAGGING, IFCAP_VLAN_MTU, IFF_ALLMULTI, IFF_BROADCAST,
    IFF_MULTICAST, IFF_PROMISC, IFF_RUNNING, IFF_SIMPLEX, IFF_UP, IFNAMSIZ, IFSFF_ADDR_DDM,
    IFSFF_ADDR_EEPROM, IFXF_MBUF_64BIT, IFXF_MPSAFE, IfSffpage, Ifmediareq, Ifreq, LINK_STATE_DOWN,
    LINK_STATE_FULL_DUPLEX, LINK_STATE_HALF_DUPLEX, if_attach, if_detach, if_link_state_change,
    if_mbps, if_rxr_get, if_rxr_init, if_rxr_ioctl, if_rxr_livelocked,
};
use crate::net::if_ethersubr::{
    ether_extract_headers, ether_ifattach, ether_ifdetach, ether_ioctl, ether_sprintf,
};
use crate::net::if_media::{
    IFM_10_T, IFM_100_TX, IFM_1000_LX, IFM_1000_SX, IFM_1000_T, IFM_ACTIVE, IFM_AUTO, IFM_AVALID,
    IFM_ETH_MASTER, IFM_ETH_RXPAUSE, IFM_ETH_TXPAUSE, IFM_ETHER, IFM_FDX, IFM_FLOW, IFM_GMASK,
    IFM_HDX, IFM_IMASK, IFM_NONE, Ifmedia, ifm_subtype, ifm_type, ifmedia_add, ifmedia_init,
    ifmedia_ioctl, ifmedia_set,
};
use crate::net::if_var::{Ifnet, if_rxr_inuse, if_rxr_needrefill, if_rxr_put};
use crate::net::ifq::{
    Ifqueue, ifiq_input, ifq_barrier, ifq_clr_oactive, ifq_dequeue, ifq_init_maxlen,
    ifq_is_oactive, ifq_purge, ifq_restart, ifq_set_oactive,
};
use crate::netinet::if_ether::{
    Arpcom, ETHER_ALIGN, ETHER_CRC_LEN, ETHER_HDR_LEN, ETHER_MAX_LEN, ETHER_MIN_LEN,
    EtherExtracted, EtherMultistep, ether_first_multi, ether_next_multi,
};
use crate::netinet::ip::Ip;
use crate::netinet::tcp_var::{TcpstatCounters, tcpstat_add, tcpstat_inc};
use crate::sys::device::{
    CfMatch, Cfattach, Cfdriver, DV_IFNET, DVACT_RESUME, DVACT_SUSPEND, Device, Softc,
};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT, M_ZERO};
use crate::sys::mbuf::{
    M_DONTWAIT, M_IPV4_CSUM_IN_OK, M_IPV4_CSUM_OUT, M_PKTHDR, M_TCP_CSUM_IN_OK, M_TCP_CSUM_OUT,
    M_TCP_TSO, M_UDP_CSUM_IN_OK, M_UDP_CSUM_OUT, M_VLANTAG, MCLBYTES, Mbuf, MbufList, mtod,
};
use crate::sys::param::PAGE_SIZE;
use crate::sys::rwlock::{RW_INTR, RW_WRITE, Rwlock};
use crate::sys::sockio::{
    SIOCGIFMEDIA, SIOCGIFRXR, SIOCGIFSFFPAGE, SIOCSIFADDR, SIOCSIFFLAGS, SIOCSIFMEDIA,
};
use crate::sys::systm::{kernel_assert_locked, kernel_lock, kernel_unlock};
use crate::sys::timeout::Timeout;

/// `EM_DRIVER_VERSION`.
pub const EM_DRIVER_VERSION: &str = "6.2.9";

// Tunables

/// `EM_MAX_TXD_82543`: the transmit descriptors of an 82542 or 82543.
pub const EM_MAX_TXD_82543: u32 = 256;
/// `EM_MAX_TXD`: the transmit descriptors of the others (`(num_tx_desc * sizeof(struct
/// em_tx_desc)) % 128 == 0`).
pub const EM_MAX_TXD: u32 = 512;
/// `EM_MAX_RXD_82543`: the receive descriptors of an 82542 or 82543.
pub const EM_MAX_RXD_82543: u32 = 256;
/// `EM_MAX_RXD`: the receive descriptors of the others.
pub const EM_MAX_RXD: u32 = 256;
/// `MAX_INTS_PER_SEC`: the Interrupt Throttle Register's limit.
pub const MAX_INTS_PER_SEC: u32 = 8000;
/// `DEFAULT_ITR`: `1/(MAX_INTS_PER_SEC * 256ns)`.
pub const DEFAULT_ITR: u32 = 1_000_000_000 / (MAX_INTS_PER_SEC * 256);
/// `EM_TIDV`: Transmit Interrupt Delay Value (units of 1.024 us).
pub const EM_TIDV: u32 = 64;
/// `EM_TADV`: Transmit Absolute Interrupt Delay Value (not on 82542/82543/82544).
pub const EM_TADV: u32 = 64;
/// `EM_RDTR`: Receive Interrupt Delay Timer; 0, as anything else may hang some adapters.
pub const EM_RDTR: u32 = 0;
/// `EM_RADV`: Receive Interrupt Absolute Delay Timer (not on 82542/82543/82544).
pub const EM_RADV: u32 = 64;
/// `EM_TX_TIMEOUT`: the transmit watchdog, in seconds.
pub const EM_TX_TIMEOUT: i16 = 5;

/// `DO_AUTO_NEG`: autonegotiation enabled.
pub const DO_AUTO_NEG: u8 = 1;
/// `WAIT_FOR_AUTO_NEG_DEFAULT`: do not wait for autonegotiation to complete.
pub const WAIT_FOR_AUTO_NEG_DEFAULT: u8 = 0;

// Tunables -- End

/// `AUTONEG_ADV_DEFAULT`.
pub const AUTONEG_ADV_DEFAULT: u16 = ADVERTISE_10_HALF
    | ADVERTISE_10_FULL
    | ADVERTISE_100_HALF
    | ADVERTISE_100_FULL
    | ADVERTISE_1000_FULL;

/// `EM_MMBA`: memory base address.
pub const EM_MMBA: i32 = 0x0010;
/// `EM_FLASH`: flash memory on ICH8.
pub const EM_FLASH: i32 = 0x0014;

/// `EM_ROUNDUP(size, unit)`: `size` rounded up to a multiple of `unit`, a power of two.
pub const fn em_roundup(size: u32, unit: u32) -> u32 {
    (size + unit - 1) & !(unit - 1)
}

/// `EM_SMARTSPEED_DOWNSHIFT`.
pub const EM_SMARTSPEED_DOWNSHIFT: u32 = 3;
/// `EM_SMARTSPEED_MAX`.
pub const EM_SMARTSPEED_MAX: u32 = 15;

/// `MAX_NUM_MULTICAST_ADDRESSES`.
pub const MAX_NUM_MULTICAST_ADDRESSES: usize = 128;

/// `PCICFG_DESC_RING_STATUS`.
pub const PCICFG_DESC_RING_STATUS: i32 = 0xe4;
/// `FLUSH_DESC_REQUIRED`.
pub const FLUSH_DESC_REQUIRED: u16 = 0x100;

/// `EM_DBA_ALIGN`: the descriptor rings' alignment (TDLEN/RDLEN are multiples of 128).
pub const EM_DBA_ALIGN: u32 = 128;

/// `SPEED_MODE_BIT`: on PCI-E MACs only.
pub const SPEED_MODE_BIT: u32 = 1 << 21;

/// `DEBUG_INIT`.
pub const DEBUG_INIT: i32 = 0;
/// `DEBUG_IOCTL`.
pub const DEBUG_IOCTL: i32 = 0;
/// `DEBUG_HW`.
pub const DEBUG_HW: i32 = 0;

/// `EM_RXBUFFER_2048`.
pub const EM_RXBUFFER_2048: u32 = 2048;
/// `EM_RXBUFFER_4096`.
pub const EM_RXBUFFER_4096: u32 = 4096;
/// `EM_RXBUFFER_8192`.
pub const EM_RXBUFFER_8192: u32 = 8192;
/// `EM_RXBUFFER_16384`.
pub const EM_RXBUFFER_16384: u32 = 16384;

/// `EM_MCLBYTES`: a receive cluster, with room to align the IP header.
pub const EM_MCLBYTES: u32 = EM_RXBUFFER_2048 + ETHER_ALIGN as u32;

/// `EM_MAX_SCATTER`.
pub const EM_MAX_SCATTER: u32 = 64;
/// `EM_TSO_SIZE`.
pub const EM_TSO_SIZE: BusSize = 65535;
/// `EM_TSO_SEG_SIZE`: max dma segment size.
pub const EM_TSO_SEG_SIZE: BusSize = 4096;

/// `EM_PBA_BYTES_SHIFT`: for the 82547 10Mb half-duplex workaround.
pub const EM_PBA_BYTES_SHIFT: u32 = 0xA;
/// `EM_TX_HEAD_ADDR_SHIFT`.
pub const EM_TX_HEAD_ADDR_SHIFT: u32 = 7;
/// `EM_PBA_TX_MASK`.
pub const EM_PBA_TX_MASK: u32 = 0xFFFF_0000;
/// `EM_FIFO_HDR`.
pub const EM_FIFO_HDR: u32 = 0x10;
/// `EM_82547_PKT_THRESH`.
pub const EM_82547_PKT_THRESH: u32 = 0x3e0;

/// `NVLAN`: `vlan(4)` is not configured (the C's `#include "vlan.h"`).
pub const NVLAN: i32 = 0;

/// `offsetof(struct tcphdr, th_sum)` (`netinet/tcp.h`, not ported).
const TCPHDR_TH_SUM: usize = 16;
/// `offsetof(struct udphdr, uh_sum)` (`netinet/udp.h`, not ported).
const UDPHDR_UH_SUM: usize = 6;

/// `struct em_packet`: a descriptor slot's packet and its DMA map.
#[repr(C)]
pub struct EmPacket {
    /// `pkt_eop`: index of the desc to watch.
    pub pkt_eop: Cell<u32>,
    /// `pkt_m`.
    pub pkt_m: Cell<Option<&'static Mbuf>>,
    /// `pkt_map`: bus_dma map for packet.
    pub pkt_map: Cell<Option<&'static BusDmamap>>,
}

impl EmPacket {
    /// `pkt->pkt_map`, which `em_setup_*_structures` created.
    pub fn map(&self) -> &'static BusDmamap {
        match self.pkt_map.get() {
            Some(m) => m,
            None => panic(format_args!("em: packet slot without a dma map")),
        }
    }
}

/// `struct em_dma_alloc`: what `em_dma_malloc` and `em_dma_free` keep of a descriptor ring.
#[repr(C)]
pub struct EmDmaAlloc {
    /// `dma_vaddr`.
    pub dma_vaddr: Cell<*mut u8>,
    /// `dma_map`.
    pub dma_map: Cell<Option<&'static BusDmamap>>,
    /// `dma_seg`.
    pub dma_seg: Cell<BusDmaSegment>,
    /// `dma_size`.
    pub dma_size: Cell<BusSize>,
    /// `dma_nseg`.
    pub dma_nseg: Cell<i32>,
}

impl EmDmaAlloc {
    /// `dma->dma_map`.
    pub fn map(&self) -> &'static BusDmamap {
        match self.dma_map.get() {
            Some(m) => m,
            None => panic(format_args!("em: descriptor ring without a dma map")),
        }
    }
}

/// `XSUM_CONTEXT_T`: the checksum offload context last loaded into the transmit ring.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim
pub enum XsumContextT {
    /// `OFFLOAD_NONE`.
    #[default]
    OFFLOAD_NONE = 0,
    /// `OFFLOAD_TCP_IP`.
    OFFLOAD_TCP_IP,
    /// `OFFLOAD_UDP_IP`.
    OFFLOAD_UDP_IP,
}

pub use XsumContextT::*;

/// `ADDRESS_LENGTH_PAIR`: for the 82544 PCI-X workaround.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AddressLengthPair {
    /// `address`.
    pub address: u64,
    /// `length`.
    pub length: u32,
}

/// `DESC_ARRAY`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DescArray {
    /// `descriptor`.
    pub descriptor: [AddressLengthPair; 4],
    /// `elements`.
    pub elements: u32,
}

/// `struct em_rx`: a queue's receive ring (`num_rx_desc` descriptors the controller
/// handles, paired with the packets at `sc_rx_pkts_ring`). Touched by the interrupt and the
/// refill timeout at `IPL_NET` on the primary CPU, and by `init`/`stop`.
#[repr(C)]
pub struct EmRx {
    /// `sc_rx_dma`: bus_dma glue for rx desc.
    pub sc_rx_dma: EmDmaAlloc,
    /// `sc_rx_desc_ring`.
    pub sc_rx_desc_ring: Cell<*mut EmRxDesc>,
    /// `sc_rx_desc_head`.
    pub sc_rx_desc_head: Cell<u32>,
    /// `sc_rx_desc_tail`.
    pub sc_rx_desc_tail: Cell<u32>,
    /// `sc_rx_pkts_ring`.
    pub sc_rx_pkts_ring: Cell<*mut EmPacket>,
    /// `sc_rx_ring`.
    pub sc_rx_ring: Cell<crate::net::if_::IfRxring>,
    /// `fmp`: the first mbuf of a multisegment packet.
    pub fmp: Cell<Option<&'static Mbuf>>,
    /// `lmp`: its last.
    pub lmp: Cell<Option<&'static Mbuf>>,
    /// `dropped_pkts`.
    pub dropped_pkts: Cell<u64>,
}

/// `struct em_tx`: a queue's transmit ring. `em_start` fills descriptors from the head (its
/// send queue serialises it), `em_txeof` reclaims them from the tail in the interrupt.
#[repr(C)]
pub struct EmTx {
    /// `sc_tx_dma`: bus_dma glue for tx desc.
    pub sc_tx_dma: EmDmaAlloc,
    /// `sc_tx_desc_ring`.
    pub sc_tx_desc_ring: Cell<*mut EmTxDesc>,
    /// `sc_tx_desc_head`.
    pub sc_tx_desc_head: AtomicU32,
    /// `sc_tx_desc_tail`.
    pub sc_tx_desc_tail: AtomicU32,
    /// `sc_tx_pkts_ring`.
    pub sc_tx_pkts_ring: Cell<*mut EmPacket>,
    /// `sc_txd_cmd`.
    pub sc_txd_cmd: Cell<u32>,
    /// `active_checksum_context`.
    pub active_checksum_context: Cell<XsumContextT>,
}

/// `struct em_queue`.
#[repr(C)]
pub struct EmQueue {
    /// `sc`.
    pub sc: Cell<*const EmSoftc>,
    /// `me`: queue index, also msix vector.
    pub me: Cell<u32>,
    /// `eims`: msix only.
    pub eims: Cell<u32>,
    /// `tag`: NULL in legacy, check `sc_intrhand`.
    pub tag: Cell<*mut c_void>,
    /// `name`.
    pub name: Cell<[u8; 8]>,
    /// `tx`.
    pub tx: EmTx,
    /// `rx`.
    pub rx: EmRx,
    /// `rx_refill`.
    pub rx_refill: Timeout,
}

impl EmQueue {
    /// `que->sc`.
    pub fn sc(&self) -> &'static EmSoftc {
        let p = self.sc.get();
        if p.is_null() {
            panic(format_args!("em: queue without its softc"));
        }
        // SAFETY: em_allocate_pci_resources points it at the softc that holds this queue;
        // softcs are never freed while their device exists.
        unsafe { &*p }
    }

    /// `&que->tx.sc_tx_pkts_ring[i]`.
    pub fn tx_pkt(&self, i: u32) -> &'static EmPacket {
        let base = self.tx.sc_tx_pkts_ring.get();
        if base.is_null() || i >= self.sc().sc_tx_slots.get() {
            panic(format_args!("em: bad tx packet slot {i}"));
        }
        // SAFETY: em_allocate_transmit_structures allocated `sc_tx_slots` zeroed slots
        // (valid all-zero); they stay until em_free_transmit_structures clears the pointer.
        unsafe { &*base.add(i as usize) }
    }

    /// `&que->rx.sc_rx_pkts_ring[i]`.
    pub fn rx_pkt(&self, i: u32) -> &'static EmPacket {
        let base = self.rx.sc_rx_pkts_ring.get();
        if base.is_null() || i >= self.sc().sc_rx_slots.get() {
            panic(format_args!("em: bad rx packet slot {i}"));
        }
        // SAFETY: as in `tx_pkt`, with em_allocate_receive_structures.
        unsafe { &*base.add(i as usize) }
    }

    /// `&que->tx.sc_tx_desc_ring[i]`, a slot of the DMA ring.
    fn txd(&self, i: u32) -> *mut EmTxDesc {
        let base = self.tx.sc_tx_desc_ring.get();
        if base.is_null() || i >= self.sc().sc_tx_slots.get() {
            panic(format_args!("em: bad tx descriptor {i}"));
        }
        // SAFETY: em_allocate_desc_rings mapped `sc_tx_slots` descriptors at `base`.
        unsafe { base.add(i as usize) }
    }

    /// `que->tx.sc_tx_desc_ring[i]`, read as the controller left it.
    fn txd_get(&self, i: u32) -> EmTxDesc {
        // SAFETY: a slot of the mapped ring (`txd`); the descriptor is plain data.
        unsafe { ptr::read_volatile(self.txd(i)) }
    }

    /// `que->tx.sc_tx_desc_ring[i] = d`.
    fn txd_set(&self, i: u32, d: EmTxDesc) {
        // SAFETY: a slot of the mapped ring (`txd`).
        unsafe { ptr::write_volatile(self.txd(i), d) }
    }

    /// `&que->rx.sc_rx_desc_ring[i]`, a slot of the DMA ring.
    fn rxd(&self, i: u32) -> *mut EmRxDesc {
        let base = self.rx.sc_rx_desc_ring.get();
        if base.is_null() || i >= self.sc().sc_rx_slots.get() {
            panic(format_args!("em: bad rx descriptor {i}"));
        }
        // SAFETY: em_allocate_desc_rings mapped `sc_rx_slots` descriptors at `base`.
        unsafe { base.add(i as usize) }
    }

    /// `que->rx.sc_rx_desc_ring[i]`, read as the controller wrote it back.
    fn rxd_get(&self, i: u32) -> EmRxDesc {
        // SAFETY: a slot of the mapped ring (`rxd`); the descriptor is plain data.
        unsafe { ptr::read_volatile(self.rxd(i)) }
    }

    /// `que->rx.sc_rx_desc_ring[i].status`.
    fn rxd_status(&self, i: u32) -> u8 {
        // SAFETY: a slot of the mapped ring (`rxd`); `status` is one byte of it.
        unsafe { ptr::read_volatile(ptr::addr_of!((*self.rxd(i)).status)) }
    }

    /// `que->rx.sc_rx_desc_ring[i] = d`.
    fn rxd_set(&self, i: u32, d: EmRxDesc) {
        // SAFETY: a slot of the mapped ring (`rxd`).
        unsafe { ptr::write_volatile(self.rxd(i), d) }
    }

    /// `que->rx.sc_rx_ring`, changed by `f`.
    fn with_rx_ring<R>(&self, f: impl FnOnce(&mut crate::net::if_::IfRxring) -> R) -> R {
        let mut ring = self.rx.sc_rx_ring.get();
        let r = f(&mut ring);
        self.rx.sc_rx_ring.set(ring);
        r
    }
}

/// `struct em_softc`: our adapter structure.
#[repr(C)]
pub struct EmSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_ac`.
    pub sc_ac: Arpcom,

    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_dmaflags`.
    pub sc_dmaflags: Cell<i32>,

    /// `hw`: the shared code's state, reached through [`EmSoftc::em_hw`] under the kernel
    /// lock; written by `em_attach` (`hw_init`).
    hw: UnsafeCell<MaybeUninit<EmHw>>,
    /// Not in the C: whether `hw` is written.
    hw_init: AtomicBool,
    /// Not in the C: whether an [`EmHwGuard`] is alive (under the kernel lock).
    hw_busy: Cell<bool>,
    /// Not in the C: the register view of `hw` (`back` and `mac_type`), written once by
    /// `em_allocate_pci_resources` before the interrupt is established (`regs_init`), then
    /// only read, from any context.
    regs: UnsafeCell<MaybeUninit<EmHw>>,
    /// Not in the C: whether `regs` is written.
    regs_init: AtomicBool,
    /// Not in the C: `hw.tbi_compatibility_on`, for `TBI_ACCEPT` in `em_rxeof`.
    tbi_compatibility_on: AtomicBool,
    /// Not in the C: `hw.min_frame_size`, for `TBI_ACCEPT`.
    min_frame_size: AtomicU32,
    /// Not in the C: `hw.max_frame_size`, for `TBI_ACCEPT`.
    max_frame_size: AtomicU32,

    /// `osdep`: OpenBSD operating-system-specific structures (the register mappings and the
    /// PCI attach arguments), written by `em_allocate_pci_resources` (`osdep_init`).
    osdep: UnsafeCell<MaybeUninit<EmOsdep>>,
    /// Not in the C: whether `osdep` is written.
    osdep_init: AtomicBool,
    /// `media`.
    pub media: Ifmedia,
    /// `io_rid`.
    pub io_rid: Cell<i32>,
    /// `legacy_irq`.
    pub legacy_irq: Cell<i32>,

    /// `sc_intrhand`.
    pub sc_intrhand: Cell<*mut c_void>,
    /// `em_intr_enable`.
    pub em_intr_enable: Timeout,
    /// `timer_handle`.
    pub timer_handle: Timeout,
    /// `tx_fifo_timer_handle`.
    pub tx_fifo_timer_handle: Timeout,

    // Info about the board itself
    /// `part_num`.
    pub part_num: Cell<u32>,
    /// `link_active`.
    pub link_active: AtomicU8,
    /// `link_speed`.
    pub link_speed: Cell<u16>,
    /// `link_duplex`.
    pub link_duplex: AtomicU16,
    /// `smartspeed`.
    pub smartspeed: Cell<u32>,
    /// `tx_int_delay`.
    pub tx_int_delay: Cell<u32>,
    /// `tx_abs_int_delay`.
    pub tx_abs_int_delay: Cell<u32>,
    /// `rx_int_delay`.
    pub rx_int_delay: Cell<u32>,
    /// `rx_abs_int_delay`.
    pub rx_abs_int_delay: Cell<u32>,
    /// `sfflock`.
    pub sfflock: Rwlock,

    /// `sc_tx_slots`.
    pub sc_tx_slots: Cell<u32>,
    /// `sc_rx_slots`.
    pub sc_rx_slots: Cell<u32>,
    /// `sc_rx_buffer_len`.
    pub sc_rx_buffer_len: Cell<u32>,

    // Misc stats maintained by the driver
    /// `mbuf_alloc_failed`.
    pub mbuf_alloc_failed: Cell<u64>,
    /// `mbuf_cluster_failed`.
    pub mbuf_cluster_failed: Cell<u64>,
    /// `no_tx_desc_avail1`.
    pub no_tx_desc_avail1: Cell<u64>,
    /// `no_tx_desc_avail2`.
    pub no_tx_desc_avail2: Cell<u64>,
    /// `no_tx_map_avail`.
    pub no_tx_map_avail: Cell<u64>,
    /// `no_tx_dma_setup`.
    pub no_tx_dma_setup: Cell<u64>,
    /// `watchdog_events`.
    pub watchdog_events: Cell<u64>,
    /// `rx_overruns`.
    pub rx_overruns: Cell<u64>,

    // These are all 82547 members for the workaround. The chip is pretty old, single
    // queue, so keep it here to avoid further changes.
    /// `tx_fifo_size`.
    pub tx_fifo_size: AtomicU32,
    /// `tx_fifo_head`.
    pub tx_fifo_head: AtomicU32,
    /// `tx_fifo_head_addr`.
    pub tx_fifo_head_addr: AtomicU32,
    /// `tx_fifo_reset_cnt`.
    pub tx_fifo_reset_cnt: AtomicU64,
    /// `tx_fifo_wrk_cnt`.
    pub tx_fifo_wrk_cnt: AtomicU64,
    /// `tx_head_addr`.
    pub tx_head_addr: AtomicU32,

    /// `pcix_82544`: for the 82544 PCI-X workaround.
    pub pcix_82544: Cell<bool>,

    /// `msix`.
    pub msix: Cell<i32>,
    /// `msix_linkvec`.
    pub msix_linkvec: Cell<u32>,
    /// `msix_linkmask`.
    pub msix_linkmask: Cell<u32>,
    /// `msix_queuesmask`.
    pub msix_queuesmask: Cell<u32>,
    /// `num_queues`.
    pub num_queues: Cell<i32>,
    /// `queues`.
    pub queues: Cell<*mut EmQueue>,
}

/// The `&mut struct em_hw` of a softc ([`EmSoftc::em_hw`]): alive while the kernel lock is
/// held; dropping it refreshes the softc's copies of what `TBI_ACCEPT` reads.
pub struct EmHwGuard<'a> {
    sc: &'a EmSoftc,
    hw: &'a mut EmHw,
}

impl EmSoftc {
    /// `&sc->sc_ac.ac_if`.
    pub fn ifp(&'static self) -> &'static Ifnet {
        &self.sc_ac.ac_if
    }

    /// `DEVNAME(sc)`.
    pub fn devname(&self) -> &str {
        self.sc_dev.xname()
    }

    /// `sc->sc_dmat`.
    pub fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => panic(format_args!("em: no dma tag")),
        }
    }

    /// `&sc->hw`, for the shared code: the caller holds the kernel lock (as the C does
    /// around every change of `sc->hw`) at `splnet`, and holds no other guard.
    pub fn em_hw(&self) -> EmHwGuard<'_> {
        kernel_assert_locked();
        if !self.hw_init.load(Ordering::Acquire) {
            panic(format_args!("em: struct em_hw used before attach"));
        }
        if self.hw_busy.replace(true) {
            panic(format_args!(
                "{}: struct em_hw borrowed twice",
                self.devname()
            ));
        }
        // SAFETY: `hw_init` says em_attach wrote the value. Every `&mut` to it comes from
        // here, under the kernel lock and at splnet (so neither another CPU nor em's own
        // interrupt handlers can make one), and `hw_busy` refuses a second one while this
        // guard lives.
        let hw = unsafe { (*self.hw.get()).assume_init_mut() };
        EmHwGuard { sc: self, hw }
    }

    /// The register view of `sc->hw` (`back` and `mac_type`), for any context.
    pub fn regs(&self) -> &EmHw {
        if !self.regs_init.load(Ordering::Acquire) {
            panic(format_args!("em: registers used before they are mapped"));
        }
        // SAFETY: `regs_init` says em_allocate_pci_resources wrote it; it is never written
        // again, so shared references are sound from any context.
        unsafe { (*self.regs.get()).assume_init_ref() }
    }

    /// `sc->hw.mac_type`, which `em_identify_hardware` sets once.
    pub fn mac_type(&self) -> EmMacType {
        self.regs().mac_type
    }

    /// `&sc->osdep`.
    pub fn osdep(&self) -> &EmOsdep {
        if !self.osdep_init.load(Ordering::Acquire) {
            panic(format_args!("em: osdep used before it is written"));
        }
        // SAFETY: `osdep_init` says em_allocate_pci_resources wrote it. Its later writes
        // (the I/O and flash mappings, the teardown) happen in the attach and the detach,
        // which hold no reference from here across them.
        unsafe { (*self.osdep.get()).assume_init_ref() }
    }

    /// `&sc->osdep`, to change it: a `&mut` made from it is sound once the osdep is
    /// written (`osdep_init`), in the attach or the detach, while no reference from
    /// [`EmSoftc::osdep`] or from the shared code's `hw.osdep()` is alive.
    fn osdep_ptr(&self) -> *mut EmOsdep {
        self.osdep.get().cast()
    }

    /// `&sc->osdep.em_pa`.
    pub fn em_pa(&self) -> &PciAttachArgs {
        &self.osdep().em_pa
    }

    /// `que`, the `i`th of `sc->queues`.
    pub fn que(&self, i: usize) -> &'static EmQueue {
        let base = self.queues.get();
        if base.is_null() || i >= self.num_queues.get() as usize {
            panic(format_args!("em: no queue {i}"));
        }
        // SAFETY: em_allocate_pci_resources allocated `num_queues` zeroed queues (valid
        // all-zero) that live until em_free_pci_resources clears the pointer.
        unsafe { &*base.add(i) }
    }

    /// `FOREACH_QUEUE(sc, que)`.
    pub fn queues(&self) -> impl Iterator<Item = &'static EmQueue> + '_ {
        let n = if self.queues.get().is_null() {
            0
        } else {
            self.num_queues.get().max(0) as usize
        };
        (0..n).map(move |i| self.que(i))
    }
}

// SAFETY: `#[repr(C)]` with the device first; the arpcom, the ifmedia, the timeouts and the
// rwlock are all-zero valid, `MaybeUninit` needs no valid bits, and every other member is a
// `Cell` or an atomic of an integer, a pointer, an `Option` or a bool.
unsafe impl Softc for EmSoftc {}

/// `em_ca`.
pub static EM_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<EmSoftc>(),
    ca_match: Some(em_probe),
    ca_attach: em_attach,
    ca_detach: Some(em_detach),
    ca_activate: Some(em_activate),
};

/// `em_cd`.
pub static EM_CD: Cfdriver = Cfdriver::new(b"em", DV_IFNET, 0);

/// `em_smart_pwr_down`: leave smart power down off on the newer adapters.
static EM_SMART_PWR_DOWN: bool = false;
/// `em_enable_msix`: MSI-X (and its link vector) on the 82576, 82580, i350 and i210, off by
/// default as in the C.
pub static EM_ENABLE_MSIX: AtomicI32 = AtomicI32::new(0);

/// The functions of this file the shared code calls (`if_em_hw.h`'s prototypes).
pub static EM_PCI_OPS: EmPciOps = EmPciOps {
    em_pci_set_mwi,
    em_pci_clear_mwi,
    em_read_pci_cfg,
    em_write_pci_cfg,
    em_read_pcie_cap_reg,
};

/// `em_devices[]`: the PCI device id table.
pub static EM_DEVICES: [PciMatchid; 198] = [
    intel(PCI_PRODUCT_INTEL_80003ES2LAN_CPR_DPT),
    intel(PCI_PRODUCT_INTEL_80003ES2LAN_SDS_DPT),
    intel(PCI_PRODUCT_INTEL_80003ES2LAN_CPR_SPT),
    intel(PCI_PRODUCT_INTEL_80003ES2LAN_SDS_SPT),
    intel(PCI_PRODUCT_INTEL_82540EM),
    intel(PCI_PRODUCT_INTEL_82540EM_LOM),
    intel(PCI_PRODUCT_INTEL_82540EP),
    intel(PCI_PRODUCT_INTEL_82540EP_LOM),
    intel(PCI_PRODUCT_INTEL_82540EP_LP),
    intel(PCI_PRODUCT_INTEL_82541EI),
    intel(PCI_PRODUCT_INTEL_82541EI_MOBILE),
    intel(PCI_PRODUCT_INTEL_82541ER),
    intel(PCI_PRODUCT_INTEL_82541ER_LOM),
    intel(PCI_PRODUCT_INTEL_82541GI),
    intel(PCI_PRODUCT_INTEL_82541GI_LF),
    intel(PCI_PRODUCT_INTEL_82541GI_MOBILE),
    intel(PCI_PRODUCT_INTEL_82542),
    intel(PCI_PRODUCT_INTEL_82543GC_COPPER),
    intel(PCI_PRODUCT_INTEL_82543GC_FIBER),
    intel(PCI_PRODUCT_INTEL_82544EI_COPPER),
    intel(PCI_PRODUCT_INTEL_82544EI_FIBER),
    intel(PCI_PRODUCT_INTEL_82544GC_COPPER),
    intel(PCI_PRODUCT_INTEL_82544GC_LOM),
    intel(PCI_PRODUCT_INTEL_82545EM_COPPER),
    intel(PCI_PRODUCT_INTEL_82545EM_FIBER),
    intel(PCI_PRODUCT_INTEL_82545GM_COPPER),
    intel(PCI_PRODUCT_INTEL_82545GM_FIBER),
    intel(PCI_PRODUCT_INTEL_82545GM_SERDES),
    intel(PCI_PRODUCT_INTEL_82546EB_COPPER),
    intel(PCI_PRODUCT_INTEL_82546EB_FIBER),
    intel(PCI_PRODUCT_INTEL_82546EB_QUAD_CPR),
    intel(PCI_PRODUCT_INTEL_82546GB_COPPER),
    intel(PCI_PRODUCT_INTEL_82546GB_FIBER),
    intel(PCI_PRODUCT_INTEL_82546GB_PCIE),
    intel(PCI_PRODUCT_INTEL_82546GB_QUAD_CPR),
    intel(PCI_PRODUCT_INTEL_82546GB_QUAD_CPR_K),
    intel(PCI_PRODUCT_INTEL_82546GB_SERDES),
    intel(PCI_PRODUCT_INTEL_82546GB_2),
    intel(PCI_PRODUCT_INTEL_82547EI),
    intel(PCI_PRODUCT_INTEL_82547EI_MOBILE),
    intel(PCI_PRODUCT_INTEL_82547GI),
    intel(PCI_PRODUCT_INTEL_82571EB_AF),
    intel(PCI_PRODUCT_INTEL_82571EB_AT),
    intel(PCI_PRODUCT_INTEL_82571EB_COPPER),
    intel(PCI_PRODUCT_INTEL_82571EB_FIBER),
    intel(PCI_PRODUCT_INTEL_82571EB_QUAD_CPR),
    intel(PCI_PRODUCT_INTEL_82571EB_QUAD_CPR_LP),
    intel(PCI_PRODUCT_INTEL_82571EB_QUAD_FBR),
    intel(PCI_PRODUCT_INTEL_82571EB_SERDES),
    intel(PCI_PRODUCT_INTEL_82571EB_SDS_DUAL),
    intel(PCI_PRODUCT_INTEL_82571EB_SDS_QUAD),
    intel(PCI_PRODUCT_INTEL_82571PT_QUAD_CPR),
    intel(PCI_PRODUCT_INTEL_82572EI_COPPER),
    intel(PCI_PRODUCT_INTEL_82572EI_FIBER),
    intel(PCI_PRODUCT_INTEL_82572EI_SERDES),
    intel(PCI_PRODUCT_INTEL_82572EI),
    intel(PCI_PRODUCT_INTEL_82573E),
    intel(PCI_PRODUCT_INTEL_82573E_IAMT),
    intel(PCI_PRODUCT_INTEL_82573E_PM),
    intel(PCI_PRODUCT_INTEL_82573L),
    intel(PCI_PRODUCT_INTEL_82573L_PL_1),
    intel(PCI_PRODUCT_INTEL_82573L_PL_2),
    intel(PCI_PRODUCT_INTEL_82573V_PM),
    intel(PCI_PRODUCT_INTEL_82574L),
    intel(PCI_PRODUCT_INTEL_82574LA),
    intel(PCI_PRODUCT_INTEL_82575EB_COPPER),
    intel(PCI_PRODUCT_INTEL_82575EB_SERDES),
    intel(PCI_PRODUCT_INTEL_82575GB_QUAD_CPR),
    intel(PCI_PRODUCT_INTEL_82575GB_QP_PM),
    intel(PCI_PRODUCT_INTEL_82576),
    intel(PCI_PRODUCT_INTEL_82576_FIBER),
    intel(PCI_PRODUCT_INTEL_82576_SERDES),
    intel(PCI_PRODUCT_INTEL_82576_QUAD_COPPER),
    intel(PCI_PRODUCT_INTEL_82576_QUAD_CU_ET2),
    intel(PCI_PRODUCT_INTEL_82576_NS),
    intel(PCI_PRODUCT_INTEL_82576_NS_SERDES),
    intel(PCI_PRODUCT_INTEL_82576_SERDES_QUAD),
    intel(PCI_PRODUCT_INTEL_82577LC),
    intel(PCI_PRODUCT_INTEL_82577LM),
    intel(PCI_PRODUCT_INTEL_82578DC),
    intel(PCI_PRODUCT_INTEL_82578DM),
    intel(PCI_PRODUCT_INTEL_82579LM),
    intel(PCI_PRODUCT_INTEL_82579V),
    intel(PCI_PRODUCT_INTEL_I210_COPPER),
    intel(PCI_PRODUCT_INTEL_I210_COPPER_OEM1),
    intel(PCI_PRODUCT_INTEL_I210_COPPER_IT),
    intel(PCI_PRODUCT_INTEL_I210_FIBER),
    intel(PCI_PRODUCT_INTEL_I210_SERDES),
    intel(PCI_PRODUCT_INTEL_I210_SGMII),
    intel(PCI_PRODUCT_INTEL_I210_COPPER_NF),
    intel(PCI_PRODUCT_INTEL_I210_SERDES_NF),
    intel(PCI_PRODUCT_INTEL_I211_COPPER),
    intel(PCI_PRODUCT_INTEL_I217_LM),
    intel(PCI_PRODUCT_INTEL_I217_V),
    intel(PCI_PRODUCT_INTEL_I218_LM),
    intel(PCI_PRODUCT_INTEL_I218_LM_2),
    intel(PCI_PRODUCT_INTEL_I218_LM_3),
    intel(PCI_PRODUCT_INTEL_I218_V),
    intel(PCI_PRODUCT_INTEL_I218_V_2),
    intel(PCI_PRODUCT_INTEL_I218_V_3),
    intel(PCI_PRODUCT_INTEL_I219_LM),
    intel(PCI_PRODUCT_INTEL_I219_LM2),
    intel(PCI_PRODUCT_INTEL_I219_LM3),
    intel(PCI_PRODUCT_INTEL_I219_LM4),
    intel(PCI_PRODUCT_INTEL_I219_LM5),
    intel(PCI_PRODUCT_INTEL_I219_LM6),
    intel(PCI_PRODUCT_INTEL_I219_LM7),
    intel(PCI_PRODUCT_INTEL_I219_LM8),
    intel(PCI_PRODUCT_INTEL_I219_LM9),
    intel(PCI_PRODUCT_INTEL_I219_LM10),
    intel(PCI_PRODUCT_INTEL_I219_LM11),
    intel(PCI_PRODUCT_INTEL_I219_LM12),
    intel(PCI_PRODUCT_INTEL_I219_LM13),
    intel(PCI_PRODUCT_INTEL_I219_LM14),
    intel(PCI_PRODUCT_INTEL_I219_LM15),
    intel(PCI_PRODUCT_INTEL_I219_LM16),
    intel(PCI_PRODUCT_INTEL_I219_LM17),
    intel(PCI_PRODUCT_INTEL_I219_LM18),
    intel(PCI_PRODUCT_INTEL_I219_LM19),
    intel(PCI_PRODUCT_INTEL_I219_LM20),
    intel(PCI_PRODUCT_INTEL_I219_LM21),
    intel(PCI_PRODUCT_INTEL_I219_LM22),
    intel(PCI_PRODUCT_INTEL_I219_LM23),
    intel(PCI_PRODUCT_INTEL_I219_LM24),
    intel(PCI_PRODUCT_INTEL_I219_LM25),
    intel(PCI_PRODUCT_INTEL_I219_LM27),
    intel(PCI_PRODUCT_INTEL_I219_V),
    intel(PCI_PRODUCT_INTEL_I219_V2),
    intel(PCI_PRODUCT_INTEL_I219_V4),
    intel(PCI_PRODUCT_INTEL_I219_V5),
    intel(PCI_PRODUCT_INTEL_I219_V6),
    intel(PCI_PRODUCT_INTEL_I219_V7),
    intel(PCI_PRODUCT_INTEL_I219_V8),
    intel(PCI_PRODUCT_INTEL_I219_V9),
    intel(PCI_PRODUCT_INTEL_I219_V10),
    intel(PCI_PRODUCT_INTEL_I219_V11),
    intel(PCI_PRODUCT_INTEL_I219_V12),
    intel(PCI_PRODUCT_INTEL_I219_V13),
    intel(PCI_PRODUCT_INTEL_I219_V14),
    intel(PCI_PRODUCT_INTEL_I219_V15),
    intel(PCI_PRODUCT_INTEL_I219_V16),
    intel(PCI_PRODUCT_INTEL_I219_V17),
    intel(PCI_PRODUCT_INTEL_I219_V18),
    intel(PCI_PRODUCT_INTEL_I219_V19),
    intel(PCI_PRODUCT_INTEL_I219_V20),
    intel(PCI_PRODUCT_INTEL_I219_V21),
    intel(PCI_PRODUCT_INTEL_I219_V22),
    intel(PCI_PRODUCT_INTEL_I219_V23),
    intel(PCI_PRODUCT_INTEL_I219_V24),
    intel(PCI_PRODUCT_INTEL_I219_V25),
    intel(PCI_PRODUCT_INTEL_I219_V27),
    intel(PCI_PRODUCT_INTEL_82580_COPPER),
    intel(PCI_PRODUCT_INTEL_82580_FIBER),
    intel(PCI_PRODUCT_INTEL_82580_SERDES),
    intel(PCI_PRODUCT_INTEL_82580_SGMII),
    intel(PCI_PRODUCT_INTEL_82580_COPPER_DUAL),
    intel(PCI_PRODUCT_INTEL_82580_QUAD_FIBER),
    intel(PCI_PRODUCT_INTEL_DH89XXCC_SGMII),
    intel(PCI_PRODUCT_INTEL_DH89XXCC_SERDES),
    intel(PCI_PRODUCT_INTEL_DH89XXCC_BPLANE),
    intel(PCI_PRODUCT_INTEL_DH89XXCC_SFP),
    intel(PCI_PRODUCT_INTEL_82583V),
    intel(PCI_PRODUCT_INTEL_I350_COPPER),
    intel(PCI_PRODUCT_INTEL_I350_FIBER),
    intel(PCI_PRODUCT_INTEL_I350_SERDES),
    intel(PCI_PRODUCT_INTEL_I350_SGMII),
    intel(PCI_PRODUCT_INTEL_I354_BP_1GBPS),
    intel(PCI_PRODUCT_INTEL_I354_BP_2_5GBPS),
    intel(PCI_PRODUCT_INTEL_I354_SGMII),
    intel(PCI_PRODUCT_INTEL_ICH8_82567V_3),
    intel(PCI_PRODUCT_INTEL_ICH8_IFE),
    intel(PCI_PRODUCT_INTEL_ICH8_IFE_G),
    intel(PCI_PRODUCT_INTEL_ICH8_IFE_GT),
    intel(PCI_PRODUCT_INTEL_ICH8_IGP_AMT),
    intel(PCI_PRODUCT_INTEL_ICH8_IGP_C),
    intel(PCI_PRODUCT_INTEL_ICH8_IGP_M),
    intel(PCI_PRODUCT_INTEL_ICH8_IGP_M_AMT),
    intel(PCI_PRODUCT_INTEL_ICH9_BM),
    intel(PCI_PRODUCT_INTEL_ICH9_IFE),
    intel(PCI_PRODUCT_INTEL_ICH9_IFE_G),
    intel(PCI_PRODUCT_INTEL_ICH9_IFE_GT),
    intel(PCI_PRODUCT_INTEL_ICH9_IGP_AMT),
    intel(PCI_PRODUCT_INTEL_ICH9_IGP_C),
    intel(PCI_PRODUCT_INTEL_ICH9_IGP_M),
    intel(PCI_PRODUCT_INTEL_ICH9_IGP_M_AMT),
    intel(PCI_PRODUCT_INTEL_ICH9_IGP_M_V),
    intel(PCI_PRODUCT_INTEL_ICH10_D_BM_LF),
    intel(PCI_PRODUCT_INTEL_ICH10_D_BM_LM),
    intel(PCI_PRODUCT_INTEL_ICH10_D_BM_V),
    intel(PCI_PRODUCT_INTEL_ICH10_R_BM_LF),
    intel(PCI_PRODUCT_INTEL_ICH10_R_BM_LM),
    intel(PCI_PRODUCT_INTEL_ICH10_R_BM_V),
    intel(PCI_PRODUCT_INTEL_EP80579_LAN_1),
    intel(PCI_PRODUCT_INTEL_EP80579_LAN_2),
    intel(PCI_PRODUCT_INTEL_EP80579_LAN_3),
    intel(PCI_PRODUCT_INTEL_EP80579_LAN_4),
    intel(PCI_PRODUCT_INTEL_EP80579_LAN_5),
    intel(PCI_PRODUCT_INTEL_EP80579_LAN_6),
];

/// `EM_TX_OP_THRESHOLD`: the free transmit descriptors needed before a packet is tried.
pub fn em_tx_op_threshold(sc: &EmSoftc) -> u32 {
    sc.sc_tx_slots.get() / 32
}

impl Deref for EmHwGuard<'_> {
    type Target = EmHw;

    fn deref(&self) -> &EmHw {
        self.hw
    }
}

impl DerefMut for EmHwGuard<'_> {
    fn deref_mut(&mut self) -> &mut EmHw {
        self.hw
    }
}

impl Drop for EmHwGuard<'_> {
    fn drop(&mut self) {
        let sc = self.sc;
        sc.tbi_compatibility_on
            .store(self.hw.tbi_compatibility_on, Ordering::Relaxed);
        sc.min_frame_size
            .store(self.hw.min_frame_size, Ordering::Relaxed);
        sc.max_frame_size
            .store(self.hw.max_frame_size, Ordering::Relaxed);
        sc.hw_busy.set(false);
    }
}

/// `(struct em_softc *)ifp->if_softc`.
pub fn em_softc(ifp: &Ifnet) -> &'static EmSoftc {
    let p = ifp.if_softc.get().cast::<EmSoftc>().cast_const();
    if p.is_null() {
        panic(format_args!("em: interface without its softc"));
    }
    // SAFETY: em_setup_interface sets `if_softc` to its softc and installs em's functions
    // only on its own interface; softcs outlive their interfaces.
    unsafe { &*p }
}

/// `{ PCI_VENDOR_INTEL, product }` of `em_devices[]`.
const fn intel(product: u32) -> PciMatchid {
    PciMatchid {
        pm_vid: PCI_VENDOR_INTEL as PciVendorId,
        pm_pid: product as PciProductId,
    }
}

/// `intr_establish`'s name for a queue's MSI-X vector: a copy of `name`, up to its NUL, that
/// lives as long as the kernel (`evcount(9)` keeps the pointer; the queue may be freed).
fn em_intr_name(name: &[u8]) -> &'static str {
    let n = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let Some(p) = malloc(n.max(1), M_DEVBUF, M_NOWAIT) else {
        return "em";
    };
    // SAFETY: a fresh allocation of at least `n` bytes that is never freed, so the copy may
    // be borrowed for the rest of the kernel's life.
    let copy: &'static mut [u8] = unsafe { core::slice::from_raw_parts_mut(p.as_ptr(), n) };
    copy.copy_from_slice(&name[..n]);
    core::str::from_utf8(copy).unwrap_or("em")
}

/// The softc of a device made for `em_ca`.
fn em_sc(self_: &Device) -> &'static EmSoftc {
    // SAFETY: `self_` was made for `em_ca`, whose softc is an `EmSoftc`; softcs are never
    // freed while the device exists, so the softc may be borrowed for 'static.
    unsafe { &*ptr::from_ref(self_.softc::<EmSoftc>()) }
}

// OpenBSD Device Interface Entry Points

/// `em_probe`: whether the adapter is one of `em_devices[]`.
pub fn em_probe(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    pci_matchbyid(pa, &EM_DEVICES)
}

/// `em_defer_attach`: the rest of an EP80579 attach, once its GCU (the MDIO bus) is there.
pub fn em_defer_attach(self_: &Device) {
    let sc = em_sc(self_);
    let pc = sc.em_pa().pa_pc;

    let Some(gcu) = em_lookup_gcu(self_) else {
        printf(format_args!(
            "{}: No GCU found, deferred attachment failed\n",
            sc.devname()
        ));

        if let Some(ih) = NonNull::new(sc.sc_intrhand.replace(ptr::null_mut())) {
            // SAFETY: the handler em_allocate_msix/em_allocate_legacy established; its
            // pointer is gone from the softc.
            unsafe { pci_intr_disestablish(pc, ih) };
        }

        em_stop(sc, true);

        em_free_pci_resources(sc);

        return;
    };

    sc.em_hw().gcu = Some(gcu);

    em_attach_miibus(self_);

    em_setup_interface(sc);

    let _ = em_setup_link(&mut sc.em_hw());

    em_update_link_status(sc);
}

/// `em_attach`: identifies the hardware, allocates the resources and initializes the
/// hardware.
pub fn em_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = em_sc(self_);
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let arg: *mut c_void = ptr::from_ref(sc).cast_mut().cast();

    sc.sc_dmat.set(Some(pa.pa_dmat));
    // sc->osdep.em_pa = *pa: kept here until em_allocate_pci_resources writes the osdep.
    let mut em_pa = *pa;

    // The shared code's state, whose `back` is the softc's osdep (see the deviations).
    let back: NonNull<EmOsdep> = NonNull::from(&sc.osdep).cast();
    // SAFETY: `back` points into this softc, which lives as long as `hw`; nothing runs on
    // `hw` that reaches the osdep before em_allocate_pci_resources writes it. This attach is
    // the only writer of `hw` before `hw_init` is set.
    unsafe { (*sc.hw.get()).write(EmHw::new(back, &EM_PCI_OPS)) };
    sc.hw_init.store(true, Ordering::Release);

    timeout_set(&sc.timer_handle, em_local_timer, arg);
    timeout_set(&sc.tx_fifo_timer_handle, em_82547_move_tail, arg);

    rw_init(&sc.sfflock, "emsff");

    // Determine hardware revision
    em_identify_hardware(sc, &em_pa);
    let mac_type = sc.mac_type();

    // Only use MSI on the newer PCIe parts, with the exception of 82571/82572 due to "Byte
    // Enables 2 and 3 Are Not Set" errata
    if mac_type <= em_82572 {
        em_pa.pa_flags &= !PCI_FLAGS_MSI_ENABLED;
    }

    // Parameters (to be read from user)
    if mac_type >= em_82544 {
        sc.sc_tx_slots.set(EM_MAX_TXD);
        sc.sc_rx_slots.set(EM_MAX_RXD);
    } else {
        sc.sc_tx_slots.set(EM_MAX_TXD_82543);
        sc.sc_rx_slots.set(EM_MAX_RXD_82543);
    }
    sc.tx_int_delay.set(EM_TIDV);
    sc.tx_abs_int_delay.set(EM_TADV);
    sc.rx_int_delay.set(EM_RDTR);
    sc.rx_abs_int_delay.set(EM_RADV);
    {
        let mut hw = sc.em_hw();
        hw.autoneg = DO_AUTO_NEG;
        hw.wait_autoneg_complete = WAIT_FOR_AUTO_NEG_DEFAULT;
        hw.autoneg_advertised = AUTONEG_ADV_DEFAULT;
        hw.tbi_compatibility_en = true;
    }
    sc.sc_rx_buffer_len.set(EM_RXBUFFER_2048);

    {
        let mut hw = sc.em_hw();
        hw.phy_init_script = 1;
        hw.phy_reset_disable = false;

        // EM_MASTER_SLAVE is not defined.
        hw.master_slave = em_ms_hw_default;

        // This controls when hardware reports transmit completion status.
        hw.report_tx_early = true;
    }

    let attached = 'err_pci: {
        if em_allocate_pci_resources(sc, &em_pa).is_err() {
            break 'err_pci false;
        }

        // Initialize eeprom parameters
        let _ = em_init_eeprom_params(&mut sc.em_hw());

        // Set the max frame size assuming standard Ethernet sized frames.
        {
            let mut hw = sc.em_hw();
            hw.max_frame_size = match hw.mac_type {
                em_82573 => {
                    let mut eeprom_data = [0u16; 1];

                    // 82573 only supports Jumbo frames if ASPM is disabled.
                    let _ = em_read_eeprom(&mut hw, EEPROM_INIT_3GIO_3, 1, &mut eeprom_data);
                    if eeprom_data[0] & EEPROM_WORD1A_ASPM_MASK != 0 {
                        ETHER_MAX_LEN as u32
                    } else {
                        // Allow Jumbo frames: 9K Jumbo Frame size
                        9234
                    }
                }
                em_82571 | em_82572 | em_82574 | em_82575 | em_82576 | em_82580 | em_i210
                | em_i350 | em_ich9lan | em_ich10lan | em_pch2lan | em_pch_lpt | em_pch_spt
                | em_pch_cnp | em_pch_tgp | em_pch_adp | em_pch_mtp | em_pch_ptp
                | em_80003es2lan => {
                    // 9K Jumbo Frame size
                    9234
                }
                em_pchlan => 4096,
                em_82542_rev2_0 | em_82542_rev2_1 | em_ich8lan => {
                    // Adapters that do not support Jumbo frames
                    ETHER_MAX_LEN as u32
                }
                _ => MAX_JUMBO_FRAME_SIZE,
            };

            hw.min_frame_size = (ETHER_MIN_LEN + ETHER_CRC_LEN) as u32;

            em_get_bus_info(&mut hw);
            if hw.bus_type == em_bus_type_pci_express {
                sc.sc_dmaflags.set(sc.sc_dmaflags.get() | BUS_DMA_64BIT);
            }
        }

        if em_allocate_desc_rings(sc).is_err() {
            printf(format_args!(
                "{}: Unable to allocate descriptor ring memory\n",
                sc.devname()
            ));
            break 'err_pci false;
        }

        // Initialize the hardware
        let mut defer = false;
        match em_hardware_init(sc) {
            Ok(()) => {}
            Err(Errno::EAGAIN) => {
                defer = true;
                config_defer(self_, em_defer_attach);
            }
            Err(_) => {
                printf(format_args!(
                    "{}: Unable to initialize the hardware\n",
                    sc.devname()
                ));
                break 'err_pci false;
            }
        }

        if matches!(
            mac_type,
            em_80003es2lan | em_82575 | em_82576 | em_82580 | em_i210 | em_i350
        ) {
            let reg = em_read_reg(sc.regs(), E1000_STATUS);
            let mut hw = sc.em_hw();
            hw.bus_func = ((reg & E1000_STATUS_FUNC_MASK) >> E1000_STATUS_FUNC_SHIFT) as u8;

            match hw.bus_func {
                0 => hw.swfw = E1000_SWFW_PHY0_SM,
                1 => hw.swfw = E1000_SWFW_PHY1_SM,
                2 => hw.swfw = E1000_SWFW_PHY2_SM,
                3 => hw.swfw = E1000_SWFW_PHY3_SM,
                _ => {}
            }
        } else {
            sc.em_hw().bus_func = 0;
        }

        // Copy the permanent MAC address out of the EEPROM
        if matches!(em_read_mac_addr(&mut sc.em_hw()), Err(v) if v < 0) {
            printf(format_args!(
                "{}: EEPROM read error while reading mac address\n",
                sc.devname()
            ));
            break 'err_pci false;
        }

        let mac_addr = sc.em_hw().mac_addr;
        sc.sc_ac.ac_enaddr.set(mac_addr);

        // Setup OS specific network interface
        if !defer {
            em_setup_interface(sc);
        }

        // Initialize statistics
        em_clear_hw_cntrs(&mut sc.em_hw());
        // NKSTAT > 0: em_kstat_attach(sc); kstat(4) is not configured.
        sc.em_hw().get_link_status = true;
        if !defer {
            em_update_link_status(sc);
        }

        // EM_DEBUG is not defined: no ", mac %#x phy %#x".
        printf(format_args!(
            ", address {}\n",
            Str(&ether_sprintf(&sc.sc_ac.ac_enaddr.get()))
        ));
        // Indicate SOL/IDER usage
        if em_check_phy_reset_block(&sc.em_hw()).is_err() {
            printf(format_args!(
                "{}: PHY reset is blocked due to SOL/IDER session.\n",
                sc.devname()
            ));
        }

        // Identify 82544 on PCI-X
        let pcix_82544 = {
            let hw = sc.em_hw();
            hw.bus_type == em_bus_type_pcix && hw.mac_type == em_82544
        };
        sc.pcix_82544.set(pcix_82544);

        sc.em_hw().icp_xxxx_is_link_up = false;

        true
    };

    if !attached {
        // err_pci:
        em_free_pci_resources(sc);
    }
}

// Transmit entry point

/// `em_start`: called by the stack to initiate a transmit. It stays as long as there are
/// packets to transmit and transmit resources are available; otherwise the send queue is
/// marked active and the packet waits there.
pub fn em_start(ifq: &'static Ifqueue) {
    let Some(ifp) = ifq.ifq_if.get() else {
        return;
    };
    let sc = em_softc(ifp);
    let que = sc.que(0); // Use only first queue.
    let regs = sc.regs();
    let mac_type = sc.mac_type();
    let mut post = false;

    if sc.link_active.load(Ordering::Relaxed) == 0 {
        ifq_purge(ifq);
        return;
    }

    // calculate free space
    let head = que.tx.sc_tx_desc_head.load(Ordering::Relaxed);
    let mut free = que.tx.sc_tx_desc_tail.load(Ordering::Acquire);
    if free <= head {
        free += sc.sc_tx_slots.get();
    }
    free -= head;

    if mac_type != em_82547 {
        let map = que.tx.sc_tx_dma.map();
        bus_dmamap_sync(
            sc.dmat(),
            map,
            0,
            map.dm_mapsize.get(),
            BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
        );
    }

    loop {
        // use 2 because cksum setup can use an extra slot
        if EM_MAX_SCATTER + 2 > free {
            ifq_set_oactive(ifq);
            break;
        }

        let Some(m) = ifq_dequeue(ifq) else {
            break;
        };

        let used = em_encap(que, m);
        if used == 0 {
            m_freem(m);
            continue;
        }

        kassert!(used <= free);

        free -= used;

        // Send a copy of the frame to the BPF listener
        let bpf = ifp.if_bpf.get();
        if !bpf.is_null() {
            let _ = bpf_mtap_ether(bpf, m, BPF_DIRECTION_OUT);
        }

        // Set timeout in case hardware has problems transmitting
        ifp.if_timer.set(EM_TX_TIMEOUT);

        if mac_type == em_82547 {
            let len = m.m_pkthdr().len.get();

            if sc.link_duplex.load(Ordering::Relaxed) == HALF_DUPLEX {
                em_82547_move_tail_locked(sc);
            } else {
                e1000_write_reg(
                    regs,
                    e1000_tdt(que.me.get()),
                    que.tx.sc_tx_desc_head.load(Ordering::Relaxed),
                );
                em_82547_update_fifo_head(sc, len);
            }
        }

        post = true;
    }

    if mac_type != em_82547 {
        let map = que.tx.sc_tx_dma.map();
        bus_dmamap_sync(
            sc.dmat(),
            map,
            0,
            map.dm_mapsize.get(),
            BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
        );
        // Advance the Transmit Descriptor Tail (Tdt), this tells the E1000 that this frame
        // is available to transmit.
        if post {
            e1000_write_reg(
                regs,
                e1000_tdt(que.me.get()),
                que.tx.sc_tx_desc_head.load(Ordering::Relaxed),
            );
        }
    }
}

// Ioctl entry point

/// `em_ioctl`: called when the user wants to configure the interface.
///
/// # Safety
///
/// As for `IfIoctlFn` (`net/if_var.rs`): `data` is the kernel copy of the request the
/// command encodes.
pub unsafe fn em_ioctl(ifp: &'static Ifnet, command: u64, data: *mut u8) -> Result<(), Errno> {
    let sc = em_softc(ifp);
    let mut error: Result<(), Errno> = Ok(());

    let s = splnet();

    match command {
        SIOCSIFADDR => {
            if ifp.if_flags.get() & IFF_UP == 0 {
                ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
                em_init(sc);
            }
        }
        SIOCSIFFLAGS => {
            if ifp.if_flags.get() & IFF_UP != 0 {
                if ifp.if_flags.get() & IFF_RUNNING != 0 {
                    error = Err(Errno::ENETRESET);
                } else {
                    em_init(sc);
                }
            } else if ifp.if_flags.get() & IFF_RUNNING != 0 {
                em_stop(sc, false);
            }
        }
        SIOCSIFMEDIA | SIOCGIFMEDIA => {
            // Check SOL/IDER usage
            if command == SIOCSIFMEDIA && em_check_phy_reset_block(&sc.em_hw()).is_err() {
                printf(format_args!(
                    "{}: Media change is blocked due to SOL/IDER session.\n",
                    sc.devname()
                ));
            } else {
                // SAFETY: SIOCSIFMEDIA carries a `struct ifreq` and SIOCGIFMEDIA a `struct
                // ifmediareq` (this function's contract).
                error = unsafe { ifmedia_ioctl(ifp, data, &sc.media, command) };
            }
        }
        SIOCGIFRXR => {
            // SAFETY: SIOCGIFRXR carries a `struct ifreq` (the caller's contract).
            let ifr = unsafe { &*data.cast::<Ifreq>() };
            let ring = sc.que(0).rx.sc_rx_ring.get();
            error = if_rxr_ioctl(ifr.ifr_data() as usize, None, EM_MCLBYTES, &ring);
        }
        SIOCGIFSFFPAGE => {
            error = rw_enter(&sc.sfflock, RW_WRITE | RW_INTR);
            if error.is_ok() {
                // SAFETY: SIOCGIFSFFPAGE carries a `struct if_sffpage` (the caller's
                // contract), byte-aligned.
                let sff = unsafe { &mut *data.cast::<IfSffpage>() };
                error = em_get_sffpage(sc, sff);
                rw_exit(&sc.sfflock);
            }
        }
        _ => {
            // SAFETY: the caller's contract, forwarded.
            error = unsafe { ether_ioctl(ifp, &sc.sc_ac, command, data) };
        }
    }

    if error == Err(Errno::ENETRESET) {
        if ifp.if_flags.get() & IFF_RUNNING != 0 {
            em_disable_intr(sc);
            em_iff(sc);
            if sc.mac_type() == em_82542_rev2_0 {
                em_initialize_receive_unit(sc);
            }
            em_enable_intr(sc);
        }
        error = Ok(());
    }

    splx(s);
    error
}

// Watchdog entry point

/// `em_watchdog`: called whenever hardware quits transmitting.
pub fn em_watchdog(ifp: &'static Ifnet) {
    let sc = em_softc(ifp);
    let que = sc.que(0); // Use only first queue.
    let regs = sc.regs();

    // If we are in this routine because of pause frames, then don't reset the hardware.
    if e1000_read_reg(regs, E1000_STATUS) & E1000_STATUS_TXOFF != 0 {
        ifp.if_timer.set(EM_TX_TIMEOUT);
        return;
    }
    printf(format_args!(
        "{}: watchdog: head {} tail {} TDH {} TDT {}\n",
        sc.devname(),
        que.tx.sc_tx_desc_head.load(Ordering::Relaxed),
        que.tx.sc_tx_desc_tail.load(Ordering::Relaxed),
        e1000_read_reg(regs, e1000_tdh(que.me.get())),
        e1000_read_reg(regs, e1000_tdt(que.me.get()))
    ));

    em_init(sc);

    sc.watchdog_events.set(sc.watchdog_events.get() + 1);
}

// Init entry point

/// `em_init`: the stack's init entry point, and the driver's way back to a consistent
/// state.
pub fn em_init(sc: &'static EmSoftc) {
    let ifp = sc.ifp();
    let regs = sc.regs();
    let mac_type = sc.mac_type();

    let s = splnet();

    em_stop(sc, false);

    // Packet Buffer Allocation (PBA)
    // Writing PBA sets the receive portion of the buffer the remainder is used for the
    // transmit buffer.
    //
    // Devices before the 82547 had a Packet Buffer of 64K.
    //   Default allocation: PBA=48K for Rx, leaving 16K for Tx.
    // After the 82547 the buffer was reduced to 40K.
    //   Default allocation: PBA=30K for Rx, leaving 10K for Tx.
    //   Note: default does not leave enough room for Jumbo Frame >10k.
    let max_frame_size = sc.em_hw().max_frame_size;
    let pba = match mac_type {
        em_82547 | em_82547_rev_2 => {
            // 82547: Total Packet Buffer is 40K
            let pba = if max_frame_size > EM_RXBUFFER_8192 {
                E1000_PBA_22K // 22K for Rx, 18K for Tx
            } else {
                E1000_PBA_30K // 30K for Rx, 10K for Tx
            };
            sc.tx_fifo_head.store(0, Ordering::Relaxed);
            sc.tx_head_addr
                .store(pba << EM_TX_HEAD_ADDR_SHIFT, Ordering::Relaxed);
            sc.tx_fifo_size.store(
                (E1000_PBA_40K - pba) << EM_PBA_BYTES_SHIFT,
                Ordering::Relaxed,
            );
            pba
        }
        // Total Packet Buffer on these is 48k
        em_82571 | em_82572 | em_82575 | em_82576 | em_82580 | em_80003es2lan | em_i350 => {
            E1000_PBA_32K // 32K for Rx, 16K for Tx
        }
        em_i210 => E1000_PBA_34K,
        em_82573 => {
            // 82573: Total Packet Buffer is 32K
            // Jumbo frames not supported
            E1000_PBA_12K // 12K for Rx, 20K for Tx
        }
        em_82574 => {
            // Total Packet Buffer is 40k
            E1000_PBA_20K // 20K for Rx, 20K for Tx
        }
        em_ich8lan => E1000_PBA_8K,
        em_ich9lan | em_ich10lan => {
            // Boost Receive side for jumbo frames
            if max_frame_size > EM_RXBUFFER_4096 {
                E1000_PBA_14K
            } else {
                E1000_PBA_10K
            }
        }
        em_pchlan | em_pch2lan | em_pch_lpt | em_pch_spt | em_pch_cnp | em_pch_tgp | em_pch_adp
        | em_pch_mtp | em_pch_ptp => E1000_PBA_26K,
        _ => {
            // Devices before 82547 had a Packet Buffer of 64K.
            if max_frame_size > EM_RXBUFFER_8192 {
                E1000_PBA_40K // 40K for Rx, 24K for Tx
            } else {
                E1000_PBA_48K // 48K for Rx, 16K for Tx
            }
        }
    };
    e1000_write_reg(regs, E1000_PBA, pba);

    // Get the latest mac address, User can use a LAA
    sc.em_hw().mac_addr = sc.sc_ac.ac_enaddr.get();

    // Initialize the hardware
    if em_hardware_init(sc).is_err() {
        printf(format_args!(
            "{}: Unable to initialize the hardware\n",
            sc.devname()
        ));
        splx(s);
        return;
    }
    em_update_link_status(sc);

    e1000_write_reg(regs, E1000_VET, u32::from(ETHERTYPE_VLAN));
    if ifp.if_capabilities.get() & IFCAP_VLAN_HWTAGGING != 0 {
        em_enable_hw_vlans(sc);
    }

    // Prepare transmit descriptors and buffers
    if em_setup_transmit_structures(sc).is_err() {
        printf(format_args!(
            "{}: Could not setup transmit structures\n",
            sc.devname()
        ));
        em_stop(sc, false);
        splx(s);
        return;
    }
    em_initialize_transmit_unit(sc);

    // Prepare receive descriptors and buffers
    if em_setup_receive_structures(sc).is_err() {
        printf(format_args!(
            "{}: Could not setup receive structures\n",
            sc.devname()
        ));
        em_stop(sc, false);
        splx(s);
        return;
    }
    em_initialize_receive_unit(sc);

    // SMALL_KERNEL is not defined.
    if sc.msix.get() != 0 && em_setup_queues_msix(sc).is_err() {
        printf(format_args!("{}: Can't setup msix queues\n", sc.devname()));
        splx(s);
        return;
    }

    // Program promiscuous mode and multicast filters.
    em_iff(sc);

    ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);
    ifq_clr_oactive(&ifp.if_snd);

    timeout_add_sec(&sc.timer_handle, 1);
    em_clear_hw_cntrs(&mut sc.em_hw());
    em_enable_intr(sc);

    // Don't reset the phy next time init gets called
    sc.em_hw().phy_reset_disable = true;

    splx(s);
}

// Interrupt Service routine

/// `em_intr`: the INTx and MSI interrupt handler (one queue).
pub fn em_intr(arg: *mut c_void) -> i32 {
    // SAFETY: em_allocate_legacy established the handler with the softc as its argument.
    let sc = unsafe { &*arg.cast::<EmSoftc>().cast_const() };
    let que = sc.que(0); // single queue
    let ifp = sc.ifp();
    let regs = sc.regs();

    let reg_icr = e1000_read_reg(regs, E1000_ICR);
    let mut test_icr = reg_icr;
    if sc.mac_type() >= em_82571 {
        test_icr = reg_icr & E1000_ICR_INT_ASSERTED;
    }
    if test_icr == 0 {
        return 0;
    }

    if ifp.if_flags.get() & IFF_RUNNING != 0 {
        em_txeof(que);
        if em_rxeof(que) {
            em_rxrefill_locked(que);
        }
    }

    // Link status change
    if reg_icr & (E1000_ICR_RXSEQ | E1000_ICR_LSC) != 0 {
        kernel_lock();
        {
            let mut hw = sc.em_hw();
            hw.get_link_status = true;
            let _ = em_check_for_link(&mut hw);
        }
        em_update_link_status(sc);
        kernel_unlock();
    }

    1
}

// Media Ioctl callback

/// `em_media_status`: what `ifconfig` shows of the media (through `ifmedia_ioctl`).
pub fn em_media_status(ifp: &'static Ifnet, ifmr: &mut Ifmediareq) {
    let sc = em_softc(ifp);
    let mut fiber_type = IFM_1000_SX;

    let _ = em_check_for_link(&mut sc.em_hw());
    em_update_link_status(sc);

    ifmr.ifm_status = IFM_AVALID;
    ifmr.ifm_active = IFM_ETHER;

    if sc.link_active.load(Ordering::Relaxed) == 0 {
        ifmr.ifm_active |= IFM_NONE;
        return;
    }

    ifmr.ifm_status |= IFM_ACTIVE;

    let media_type = sc.em_hw().media_type;
    if media_type == em_media_type_fiber || media_type == em_media_type_internal_serdes {
        if sc.mac_type() == em_82545 {
            fiber_type = IFM_1000_LX;
        }
        ifmr.ifm_active |= fiber_type | IFM_FDX;
    } else {
        match sc.link_speed.get() {
            10 => ifmr.ifm_active |= IFM_10_T,
            100 => ifmr.ifm_active |= IFM_100_TX,
            1000 => ifmr.ifm_active |= IFM_1000_T,
            _ => {}
        }

        if sc.link_duplex.load(Ordering::Relaxed) == FULL_DUPLEX {
            ifmr.ifm_active |= em_flowstatus(sc) | IFM_FDX;
        } else {
            ifmr.ifm_active |= IFM_HDX;
        }

        if ifm_subtype(ifmr.ifm_active) == IFM_1000_T {
            let mut gsr: u16 = 0;
            let _ = em_read_phy_reg(&mut sc.em_hw(), PHY_1000T_STATUS, &mut gsr);
            if u32::from(gsr) & SR_1000T_MS_CONFIG_RES != 0 {
                ifmr.ifm_active |= IFM_ETH_MASTER;
            }
        }
    }
}

/// `em_media_change`: called when the user changes speed/duplex with `ifconfig media`
/// (through `ifmedia_ioctl`).
pub fn em_media_change(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = em_softc(ifp);
    let ifm_media = sc.media.ifm_media.get();

    if ifm_type(ifm_media) != IFM_ETHER {
        return Err(Errno::EINVAL);
    }

    {
        let mut hw = sc.em_hw();
        match ifm_subtype(ifm_media) {
            IFM_AUTO => {
                hw.autoneg = DO_AUTO_NEG;
                hw.autoneg_advertised = AUTONEG_ADV_DEFAULT;
            }
            IFM_1000_LX | IFM_1000_SX | IFM_1000_T => {
                hw.autoneg = DO_AUTO_NEG;
                hw.autoneg_advertised = ADVERTISE_1000_FULL;
            }
            IFM_100_TX => {
                hw.autoneg = 0;
                hw.autoneg_advertised = 0;
                hw.forced_speed_duplex = if ifm_media & IFM_GMASK == IFM_FDX {
                    em_100_full
                } else {
                    em_100_half
                };
            }
            IFM_10_T => {
                hw.autoneg = 0;
                hw.autoneg_advertised = 0;
                hw.forced_speed_duplex = if ifm_media & IFM_GMASK == IFM_FDX {
                    em_10_full
                } else {
                    em_10_half
                };
            }
            _ => {
                printf(format_args!("{}: Unsupported media type\n", sc.devname()));
            }
        }

        // As the speed/duplex settings may have changed we need to reset the PHY.
        hw.phy_reset_disable = false;
    }

    em_init(sc);

    Ok(())
}

/// `em_flowstatus`: the flow control the link negotiated, as media options.
pub fn em_flowstatus(sc: &EmSoftc) -> u64 {
    let mut hw = sc.em_hw();
    let mut ar: u16 = 0;
    let mut lpar: u16 = 0;

    if hw.media_type == em_media_type_fiber || hw.media_type == em_media_type_internal_serdes {
        return 0;
    }

    let _ = em_read_phy_reg(&mut hw, PHY_AUTONEG_ADV, &mut ar);
    let _ = em_read_phy_reg(&mut hw, PHY_LP_ABILITY, &mut lpar);
    if ar & NWAY_AR_PAUSE != 0 && lpar & NWAY_LPAR_PAUSE != 0 {
        IFM_FLOW | IFM_ETH_TXPAUSE | IFM_ETH_RXPAUSE
    } else if ar & NWAY_AR_PAUSE == 0
        && ar & NWAY_AR_ASM_DIR != 0
        && lpar & NWAY_LPAR_PAUSE != 0
        && lpar & NWAY_LPAR_ASM_DIR != 0
    {
        IFM_FLOW | IFM_ETH_TXPAUSE
    } else if ar & NWAY_AR_PAUSE != 0
        && ar & NWAY_AR_ASM_DIR != 0
        && lpar & NWAY_LPAR_PAUSE == 0
        && lpar & NWAY_LPAR_ASM_DIR != 0
    {
        IFM_FLOW | IFM_ETH_RXPAUSE
    } else {
        0
    }
}

/// `em_encap`: maps the mbuf to transmit descriptors; the slots used, 0 on failure.
pub fn em_encap(que: &EmQueue, m: &'static Mbuf) -> u32 {
    let sc = que.sc();
    let dmat = sc.dmat();
    let mac_type = sc.mac_type();
    let slots = sc.sc_tx_slots.get();
    let mut txd_upper: u32 = 0;
    let mut txd_lower: u32 = 0;
    let mut used: u32 = 0;

    // For 82544 Workaround
    let mut desc_array = DescArray::default();

    // get a dmamap for this packet from the next free slot
    let mut head = que.tx.sc_tx_desc_head.load(Ordering::Relaxed);
    let pkt = que.tx_pkt(head);
    let map = pkt.map();

    // SAFETY: the mbuf stays the slot's (`pkt_m`) until em_txeof or
    // em_free_transmit_structures unload the map before freeing it.
    let mut r = unsafe { bus_dmamap_load_mbuf(dmat, map, m, BUS_DMA_NOWAIT) };
    if r == Err(Errno::EFBIG) {
        r = match m_defrag(m, M_DONTWAIT) {
            // SAFETY: as above.
            Ok(()) => unsafe { bus_dmamap_load_mbuf(dmat, map, m, BUS_DMA_NOWAIT) },
            Err(e) => Err(e),
        };
    }
    if r.is_err() {
        sc.no_tx_dma_setup.set(sc.no_tx_dma_setup.get() + 1);
        return 0;
    }

    bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_PREWRITE);

    if mac_type == em_82547 {
        let ring = que.tx.sc_tx_dma.map();
        bus_dmamap_sync(
            dmat,
            ring,
            0,
            ring.dm_mapsize.get(),
            BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
        );
    }

    if mac_type >= em_82575 && mac_type <= em_i210 {
        if m.m_pkthdr().csum_flags.get() & M_TCP_TSO != 0 {
            used += em_tso_setup(que, m, head, &mut txd_upper, &mut txd_lower);
            if used == 0 {
                return used;
            }
        } else {
            used += em_tx_ctx_setup(que, m, head, &mut txd_upper, &mut txd_lower);
        }
    } else if mac_type >= em_82543 {
        used += em_transmit_checksum_setup(que, m, head, &mut txd_upper, &mut txd_lower);
    } else {
        txd_upper = 0;
        txd_lower = 0;
    }

    head += used;
    if head >= slots {
        head -= slots;
    }

    let txd_cmd = que.tx.sc_txd_cmd.get();
    let nsegs = map.dm_nsegs.get() as usize;
    let mut last = head;
    for seg in &map.dm_segs()[..nsegs] {
        let seg = seg.get();
        // If sc is 82544 and on PCI-X bus
        if sc.pcix_82544.get() {
            // Check the Address and Length combination and split the data accordingly
            let array_elements =
                em_fill_descriptors(seg.ds_addr as u64, seg.ds_len as u32, &mut desc_array);
            for d in &desc_array.descriptor[..array_elements as usize] {
                que.txd_set(
                    head,
                    EmTxDesc {
                        buffer_addr: d.address.to_le(),
                        lower: EmTxDescLower {
                            data: (txd_cmd | txd_lower | u32::from(d.length as u16)).to_le(),
                        },
                        upper: EmTxDescUpper {
                            data: txd_upper.to_le(),
                        },
                    },
                );

                last = head;
                head += 1;
                if head == slots {
                    head = 0;
                }

                used += 1;
            }
        } else {
            que.txd_set(
                head,
                EmTxDesc {
                    buffer_addr: (seg.ds_addr as u64).to_le(),
                    lower: EmTxDescLower {
                        data: (txd_cmd | txd_lower | seg.ds_len as u32).to_le(),
                    },
                    upper: EmTxDescUpper {
                        data: txd_upper.to_le(),
                    },
                },
            );

            last = head;
            head += 1;
            if head == slots {
                head = 0;
            }

            used += 1;
        }
    }

    // Find out if we are in VLAN mode
    if NVLAN > 0
        && m.m_flags().get() & M_VLANTAG != 0
        && (mac_type < em_82575 || mac_type > em_i210)
    {
        let mut desc = que.txd_get(last);
        // Set the VLAN id
        let mut fields = desc.upper.fields();
        fields.special = m.m_pkthdr().ether_vtag.get().to_le();
        desc.upper = EmTxDescUpper { fields };

        // Tell hardware to add tag
        desc.lower = EmTxDescLower {
            data: desc.lower.data() | E1000_TXD_CMD_VLE.to_le(),
        };
        que.txd_set(last, desc);
    }

    // mark the packet with the mbuf and last desc slot
    pkt.pkt_m.set(Some(m));
    pkt.pkt_eop.set(last);

    que.tx.sc_tx_desc_head.store(head, Ordering::Release);

    // Last Descriptor of Packet needs End Of Packet (EOP) and Report Status (RS)
    let mut desc = que.txd_get(last);
    desc.lower = EmTxDescLower {
        data: desc.lower.data() | (E1000_TXD_CMD_EOP | E1000_TXD_CMD_RS).to_le(),
    };
    que.txd_set(last, desc);

    if mac_type == em_82547 {
        let ring = que.tx.sc_tx_dma.map();
        bus_dmamap_sync(
            dmat,
            ring,
            0,
            ring.dm_mapsize.get(),
            BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
        );
    }

    used
}

// 82547 workaround to avoid controller hang in half-duplex environment. The workaround is to
// avoid queuing a large packet that would span the internal Tx FIFO ring boundary. We need
// to reset the FIFO pointers in this case. We do that only when FIFO is quiescent.

/// `em_82547_move_tail_locked`: hands the queued packets to the 82547 one by one, as long as
/// each fits the transmit FIFO.
pub fn em_82547_move_tail_locked(sc: &EmSoftc) {
    let que = sc.que(0); // single queue chip
    let regs = sc.regs();
    let slots = sc.sc_tx_slots.get();
    let mut length: u16 = 0;

    let mut hw_tdt = e1000_read_reg(regs, e1000_tdt(que.me.get())) as u16;
    let sw_tdt = que.tx.sc_tx_desc_head.load(Ordering::Relaxed) as u16;

    while hw_tdt != sw_tdt {
        let tx_desc = que.txd_get(u32::from(hw_tdt));
        length = length.wrapping_add(tx_desc.lower.flags().length);
        let eop = tx_desc.lower.data() & E1000_TXD_CMD_EOP != 0;
        hw_tdt += 1;
        if u32::from(hw_tdt) == slots {
            hw_tdt = 0;
        }

        if eop {
            if em_82547_fifo_workaround(sc, i32::from(length)) {
                sc.tx_fifo_wrk_cnt.fetch_add(1, Ordering::Relaxed);
                timeout_add(&sc.tx_fifo_timer_handle, 1);
                break;
            }
            e1000_write_reg(regs, e1000_tdt(que.me.get()), u32::from(hw_tdt));
            em_82547_update_fifo_head(sc, i32::from(length));
            length = 0;
        }
    }
}

/// `em_82547_move_tail`: `tx_fifo_timer_handle`'s function.
pub fn em_82547_move_tail(arg: *mut c_void) {
    // SAFETY: em_attach set the timeout with the softc as its argument.
    let sc = unsafe { &*arg.cast::<EmSoftc>().cast_const() };

    let s = splnet();
    em_82547_move_tail_locked(sc);
    splx(s);
}

/// `em_82547_fifo_workaround`: whether a packet of `len` bytes must wait (the FIFO could not
/// be reset).
pub fn em_82547_fifo_workaround(sc: &EmSoftc, len: i32) -> bool {
    let fifo_pkt_len = em_roundup(len as u32 + EM_FIFO_HDR, EM_FIFO_HDR);

    if sc.link_duplex.load(Ordering::Relaxed) == HALF_DUPLEX {
        let fifo_space = sc
            .tx_fifo_size
            .load(Ordering::Relaxed)
            .wrapping_sub(sc.tx_fifo_head.load(Ordering::Relaxed));

        if fifo_pkt_len >= EM_82547_PKT_THRESH.wrapping_add(fifo_space) {
            return !em_82547_tx_fifo_reset(sc);
        }
    }

    false
}

/// `em_82547_update_fifo_head`: accounts a packet of `len` bytes in the transmit FIFO.
pub fn em_82547_update_fifo_head(sc: &EmSoftc, len: i32) {
    let fifo_pkt_len = em_roundup(len as u32 + EM_FIFO_HDR, EM_FIFO_HDR);

    // tx_fifo_head is always 16 byte aligned
    let mut head = sc.tx_fifo_head.load(Ordering::Relaxed) + fifo_pkt_len;
    let size = sc.tx_fifo_size.load(Ordering::Relaxed);
    if head >= size {
        head -= size;
    }
    sc.tx_fifo_head.store(head, Ordering::Relaxed);
}

/// `em_82547_tx_fifo_reset`: resets the transmit FIFO pointers if the FIFO is quiescent.
pub fn em_82547_tx_fifo_reset(sc: &EmSoftc) -> bool {
    let que = sc.que(0); // single queue chip
    let regs = sc.regs();

    if e1000_read_reg(regs, e1000_tdt(que.me.get()))
        == e1000_read_reg(regs, e1000_tdh(que.me.get()))
        && e1000_read_reg(regs, E1000_TDFT) == e1000_read_reg(regs, E1000_TDFH)
        && e1000_read_reg(regs, E1000_TDFTS) == e1000_read_reg(regs, E1000_TDFHS)
        && e1000_read_reg(regs, E1000_TDFPC) == 0
    {
        // Disable TX unit
        let tctl = e1000_read_reg(regs, E1000_TCTL);
        e1000_write_reg(regs, E1000_TCTL, tctl & !E1000_TCTL_EN);

        // Reset FIFO pointers
        let tx_head_addr = sc.tx_head_addr.load(Ordering::Relaxed);
        e1000_write_reg(regs, E1000_TDFT, tx_head_addr);
        e1000_write_reg(regs, E1000_TDFH, tx_head_addr);
        e1000_write_reg(regs, E1000_TDFTS, tx_head_addr);
        e1000_write_reg(regs, E1000_TDFHS, tx_head_addr);

        // Re-enable TX unit
        e1000_write_reg(regs, E1000_TCTL, tctl);
        e1000_write_flush(regs);

        sc.tx_fifo_head.store(0, Ordering::Relaxed);
        sc.tx_fifo_reset_cnt.fetch_add(1, Ordering::Relaxed);

        true
    } else {
        false
    }
}

/// `em_iff`: programs the promiscuous mode and the multicast filter.
pub fn em_iff(sc: &'static EmSoftc) {
    let ifp = sc.ifp();
    let ac = &sc.sc_ac;
    let regs = sc.regs();
    let mut mta = [0u8; MAX_NUM_MULTICAST_ADDRESSES * ETH_LENGTH_OF_ADDRESS];
    let mut i = 0;

    if sc.mac_type() == em_82542_rev2_0 {
        let mut reg_rctl = e1000_read_reg(regs, E1000_RCTL);
        {
            let hw = sc.em_hw();
            if u32::from(hw.pci_cmd_word) & CMD_MEM_WRT_INVALIDATE != 0 {
                em_pci_clear_mwi(&hw);
            }
        }
        reg_rctl |= E1000_RCTL_RST;
        e1000_write_reg(regs, E1000_RCTL, reg_rctl);
        msec_delay(5);
    }

    let mut reg_rctl = e1000_read_reg(regs, E1000_RCTL);
    reg_rctl &= !(E1000_RCTL_MPE | E1000_RCTL_UPE);
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_ALLMULTI);

    if ifp.if_flags.get() & IFF_PROMISC != 0
        || ac.ac_multirangecnt.get() > 0
        || ac.ac_multicnt.get() as usize > MAX_NUM_MULTICAST_ADDRESSES
    {
        ifp.if_flags.set(ifp.if_flags.get() | IFF_ALLMULTI);
        reg_rctl |= E1000_RCTL_MPE;
        if ifp.if_flags.get() & IFF_PROMISC != 0 {
            reg_rctl |= E1000_RCTL_UPE;
        }
    } else {
        let mut step = EtherMultistep { e_enm: None };
        let mut enm = ether_first_multi(&mut step, ac);
        while let Some(e) = enm {
            let Some(slot) = mta.get_mut(i..i + ETH_LENGTH_OF_ADDRESS) else {
                break;
            };
            slot.copy_from_slice(&e.enm_addrlo);
            i += ETH_LENGTH_OF_ADDRESS;

            enm = ether_next_multi(&mut step);
        }

        em_mc_addr_list_update(&mut sc.em_hw(), &mta[..i], ac.ac_multicnt.get() as u32, 0);
    }

    e1000_write_reg(regs, E1000_RCTL, reg_rctl);

    if sc.mac_type() == em_82542_rev2_0 {
        let mut reg_rctl = e1000_read_reg(regs, E1000_RCTL);
        reg_rctl &= !E1000_RCTL_RST;
        e1000_write_reg(regs, E1000_RCTL, reg_rctl);
        msec_delay(5);
        let hw = sc.em_hw();
        if u32::from(hw.pci_cmd_word) & CMD_MEM_WRT_INVALIDATE != 0 {
            em_pci_set_mwi(&hw);
        }
    }
}

// Timer routine

/// `em_local_timer`: checks the link (SmartSpeed) every second.
pub fn em_local_timer(arg: *mut c_void) {
    // SAFETY: em_attach set the timeout with the softc as its argument.
    let sc = unsafe { &*arg.cast::<EmSoftc>().cast_const() };

    timeout_add_sec(&sc.timer_handle, 1);

    let s = splnet();
    em_smartspeed(sc);
    splx(s);

    // NKSTAT > 0: em_kstat_read(sc->kstat) under kstat_mtx; kstat(4) is not configured.
}

/// `em_update_link_status`: follows the link up and down, sets the interface's link state
/// and baudrate.
pub fn em_update_link_status(sc: &'static EmSoftc) {
    let ifp = sc.ifp();
    let regs = sc.regs();
    let mac_type = sc.mac_type();

    let link_state = if e1000_read_reg(regs, E1000_STATUS) & E1000_STATUS_LU != 0 {
        if sc.link_active.load(Ordering::Relaxed) == 0 {
            let mut speed = sc.link_speed.get();
            let mut duplex = sc.link_duplex.load(Ordering::Relaxed);
            let _ = em_get_speed_and_duplex(&mut sc.em_hw(), &mut speed, &mut duplex);
            sc.link_speed.set(speed);
            sc.link_duplex.store(duplex, Ordering::Relaxed);
            // Check if we may set SPEED_MODE bit on PCI-E
            if speed == SPEED_1000
                && matches!(
                    mac_type,
                    em_82571 | em_82572 | em_82575 | em_82576 | em_82580
                )
            {
                let tarc0 = e1000_read_reg(regs, E1000_TARC0) | SPEED_MODE_BIT;
                e1000_write_reg(regs, E1000_TARC0, tarc0);
            }
            sc.link_active.store(1, Ordering::Relaxed);
            sc.smartspeed.set(0);
            ifp.if_baudrate.set(if_mbps(u64::from(speed)));
        }
        if sc.link_duplex.load(Ordering::Relaxed) == FULL_DUPLEX {
            LINK_STATE_FULL_DUPLEX
        } else {
            LINK_STATE_HALF_DUPLEX
        }
    } else {
        if sc.link_active.load(Ordering::Relaxed) == 1 {
            ifp.if_baudrate.set(0);
            sc.link_speed.set(0);
            sc.link_duplex.store(0, Ordering::Relaxed);
            sc.link_active.store(0, Ordering::Relaxed);
        }
        LINK_STATE_DOWN
    };
    if ifp.if_link_state.get() != link_state {
        ifp.if_link_state.set(link_state);
        if_link_state_change(ifp);
    }

    // Disable TSO for 10/100 speeds to avoid some hardware issues
    let tso_capable = mac_type >= em_82575 && mac_type <= em_i210;
    match sc.link_speed.get() {
        SPEED_10 | SPEED_100 if tso_capable => {
            ifp.if_capabilities
                .set(ifp.if_capabilities.get() & !(IFCAP_TSOv4 | IFCAP_TSOv6));
        }
        SPEED_1000 if tso_capable => {
            ifp.if_capabilities
                .set(ifp.if_capabilities.get() | IFCAP_TSOv4 | IFCAP_TSOv6);
        }
        _ => {}
    }
}

/// `em_stop`: stops all traffic on the adapter (a global reset of the MAC unless
/// `softonly`) and frees the transmit and receive buffers.
pub fn em_stop(sc: &'static EmSoftc, softonly: bool) {
    let que = sc.que(0); // Use only first queue.
    let ifp = sc.ifp();

    // Tell the stack that the interface is no longer active
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);

    timeout_del(&que.rx_refill);
    timeout_del(&sc.timer_handle);
    timeout_del(&sc.tx_fifo_timer_handle);

    if !softonly {
        em_disable_intr(sc);
    }
    if sc.mac_type() >= em_pch_spt {
        em_flush_desc_rings(sc);
    }
    if !softonly {
        let _ = em_reset_hw(&mut sc.em_hw());
    }

    if let Some(ih) = NonNull::new(sc.sc_intrhand.get()) {
        intr_barrier(ih);
    }
    ifq_barrier(&ifp.if_snd);

    kassert!(ifp.if_flags.get() & IFF_RUNNING == 0);

    ifq_clr_oactive(&ifp.if_snd);
    ifp.if_timer.set(0);

    em_free_transmit_structures(sc);
    em_free_receive_structures(sc);
}

/// `em_identify_hardware`: determines the hardware revision from the PCI attach arguments
/// (the C's `&sc->osdep.em_pa`), then makes the register view of `sc->hw`.
pub fn em_identify_hardware(sc: &EmSoftc, pa: &PciAttachArgs) {
    let (back, mac_type) = {
        let mut hw = sc.em_hw();

        // Make sure our PCI config space has the necessary stuff set
        hw.pci_cmd_word = pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_COMMAND_STATUS_REG) as u16;

        // Save off the information about this board
        hw.vendor_id = pci_vendor(pa.pa_id) as u16;
        hw.device_id = pci_product(pa.pa_id) as u16;

        let reg = pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_CLASS_REG);
        hw.revision_id = pci_revision(reg) as u8;

        let reg = pci_conf_read(pa.pa_pc, pa.pa_tag, PCI_SUBSYS_ID_REG);
        hw.subsystem_vendor_id = pci_vendor(reg) as u16;
        hw.subsystem_id = pci_product(reg) as u16;

        // Identify the MAC
        if em_set_mac_type(&mut hw).is_err() {
            printf(format_args!("{}: Unknown MAC Type\n", sc.devname()));
        }

        if hw.mac_type == em_pchlan {
            hw.revision_id = (pci_product(pa.pa_id) & 0x0f) as u8;
        }

        if matches!(
            hw.mac_type,
            em_82541 | em_82541_rev_2 | em_82547 | em_82547_rev_2
        ) {
            hw.phy_init_script = 1;
        }

        (hw.back, hw.mac_type)
    };

    // The register view (not in the C): `back` and `mac_type`, fixed from here on.
    if sc.regs_init.load(Ordering::Acquire) {
        panic(format_args!("{}: hardware identified twice", sc.devname()));
    }
    // SAFETY: `back` is the softc's osdep, as for `hw`; nothing reads `regs` before
    // `regs_init` is set, and it is written only here, once.
    let mut regs = unsafe { EmHw::new(back, &EM_PCI_OPS) };
    regs.mac_type = mac_type;
    // SAFETY: as above: the one write, before any reader.
    unsafe { (*sc.regs.get()).write(regs) };
    sc.regs_init.store(true, Ordering::Release);
}

/// `em_legacy_irq_quirk_spt`: the SPT quirk of the legacy interrupt.
pub fn em_legacy_irq_quirk_spt(sc: &EmSoftc) {
    let regs = sc.regs();

    // Legacy interrupt: SPT needs a quirk.
    if sc.mac_type() < em_pch_spt {
        return;
    }
    if sc.legacy_irq.get() == 0 {
        return;
    }

    let reg = em_read_reg(regs, E1000_FEXTNVM7) | E1000_FEXTNVM7_SIDE_CLK_UNGATE;
    em_write_reg(regs, E1000_FEXTNVM7, reg);

    let reg = em_read_reg(regs, E1000_FEXTNVM9)
        | E1000_FEXTNVM9_IOSFSB_CLKGATE_DIS
        | E1000_FEXTNVM9_IOSFSB_CLKREQ_DIS;
    em_write_reg(regs, E1000_FEXTNVM9, reg);
}

/// `em_allocate_pci_resources`: maps the BARs, writes the osdep, allocates the queue and
/// establishes the interrupt; `em_pa` is the attach arguments the osdep keeps.
pub fn em_allocate_pci_resources(sc: &'static EmSoftc, em_pa: &PciAttachArgs) -> Result<(), Errno> {
    let pa = em_pa;
    let mac_type = sc.mac_type();

    let val = pci_conf_read(pa.pa_pc, pa.pa_tag, EM_MMBA);
    if PCI_MAPREG_TYPE(val) != PCI_MAPREG_TYPE_MEM {
        printf(format_args!(": mmba is not mem space\n"));
        return Err(Errno::ENXIO);
    }
    let Ok((memt, memh, membase, memsize)) =
        pci_mapreg_map(pa, EM_MMBA, pci_mapreg_mem_type(val), 0, 0)
    else {
        printf(format_args!(": cannot find mem space\n"));
        return Err(Errno::ENXIO);
    };

    // The osdep, with the I/O and flash mappings not made yet (zero bases and sizes).
    let osdep = EmOsdep {
        mem_bus_space_tag: memt,
        mem_bus_space_handle: memh,
        io_bus_space_tag: memt,
        io_bus_space_handle: memh,
        flash_bus_space_tag: memt,
        flash_bus_space_handle: memh,
        dev: None,
        em_pa: *pa,
        em_memsize: memsize,
        em_membase: membase,
        em_iosize: 0,
        em_iobase: 0,
        em_flashsize: 0,
        em_flashbase: 0,
        em_flashoffset: 0,
    };
    if sc.osdep_init.load(Ordering::Acquire) {
        panic(format_args!("{}: resources allocated twice", sc.devname()));
    }
    // SAFETY: nothing reads the osdep before `osdep_init` is set; this is its one write.
    unsafe { (*sc.osdep.get()).write(osdep) };
    sc.osdep_init.store(true, Ordering::Release);

    match mac_type {
        em_82544 | em_82540 | em_82545 | em_82546 | em_82541 | em_82541_rev_2 => {
            // Figure out where our I/O BAR is ?
            let mut rid = PCI_MAPREG_START;
            while rid < PCI_MAPREG_END {
                let val = pci_conf_read(pa.pa_pc, pa.pa_tag, rid);
                if PCI_MAPREG_TYPE(val) == PCI_MAPREG_TYPE_IO {
                    sc.io_rid.set(rid);
                    break;
                }
                rid += 4;
                if pci_mapreg_mem_type(val) == PCI_MAPREG_MEM_TYPE_64BIT {
                    rid += 4; // skip high bits, too
                }
            }

            let Ok((iot, ioh, iobase, iosize)) = pci_mapreg_map(pa, rid, PCI_MAPREG_TYPE_IO, 0, 0)
            else {
                printf(format_args!(": cannot find i/o space\n"));
                return Err(Errno::ENXIO);
            };
            // SAFETY: the attach, before the shared code reaches the registers.
            let o = unsafe { &mut *sc.osdep_ptr() };
            o.io_bus_space_tag = iot;
            o.io_bus_space_handle = ioh;
            o.em_iobase = iobase;
            o.em_iosize = iosize;

            sc.em_hw().io_base = 0;
        }
        _ => {}
    }

    // SAFETY: as above.
    unsafe { &mut *sc.osdep_ptr() }.em_flashoffset = 0;
    // for ICH8 and family we need to find the flash memory
    if mac_type >= em_pch_spt {
        // SAFETY: as above.
        let o = unsafe { &mut *sc.osdep_ptr() };
        o.flash_bus_space_tag = o.mem_bus_space_tag;
        o.flash_bus_space_handle = o.mem_bus_space_handle;
        o.em_flashbase = 0;
        o.em_flashsize = 0;
        o.em_flashoffset = 0xe000;
    } else if is_ich8(mac_type) {
        let val = pci_conf_read(pa.pa_pc, pa.pa_tag, EM_FLASH);
        if PCI_MAPREG_TYPE(val) != PCI_MAPREG_TYPE_MEM {
            printf(format_args!(": flash is not mem space\n"));
            return Err(Errno::ENXIO);
        }

        let Ok((ft, fh, fbase, fsize)) =
            pci_mapreg_map(pa, EM_FLASH, pci_mapreg_mem_type(val), 0, 0)
        else {
            printf(format_args!(": cannot find mem space\n"));
            return Err(Errno::ENXIO);
        };
        // SAFETY: as above.
        let o = unsafe { &mut *sc.osdep_ptr() };
        o.flash_bus_space_tag = ft;
        o.flash_bus_space_handle = fh;
        o.em_flashbase = fbase;
        o.em_flashsize = fsize;
    }

    // SAFETY: as above.
    unsafe { &mut *sc.osdep_ptr() }.dev = Some(NonNull::from(&sc.sc_dev));
    // sc->hw.back = &sc->osdep: em_attach made `hw` with it.

    // Only one queue for the moment.
    let Some(p) = malloc(size_of::<EmQueue>(), M_DEVBUF, M_NOWAIT | M_ZERO) else {
        printf(format_args!(": unable to allocate queue memory\n"));
        return Err(Errno::ENOMEM);
    };
    let que_p = p.cast::<EmQueue>();
    // SAFETY: a fresh zeroed allocation of an `EmQueue`, which is valid all-zero; it lives
    // until em_free_pci_resources frees it.
    let que: &'static EmQueue = unsafe { que_p.as_ref() };
    que.me.set(0);
    que.sc.set(sc);
    timeout_set(&que.rx_refill, em_rxrefill, que_p.as_ptr().cast());

    sc.queues.set(que_p.as_ptr());
    sc.num_queues.set(1);
    sc.msix.set(0);
    sc.legacy_irq.set(0);
    if em_allocate_msix(sc).is_err() && em_allocate_legacy(sc).is_err() {
        return Err(Errno::ENXIO);
    }

    // the ICP_xxxx device has multiple, duplicate register sets for use when it is being
    // used as a network processor. Disable those registers here, as they are not necessary
    // in this context and can confuse the system
    if mac_type == em_icp_xxxx {
        let pa = sc.em_pa();
        let Some((offset, _)) = pci_get_capability(pa.pa_pc, pa.pa_tag, PCI_CAP_ID_ST as i32)
        else {
            return Ok(());
        };
        let offset = offset + PCI_ST_SMIA_OFFSET as i32;
        pci_conf_write(pa.pa_pc, pa.pa_tag, offset, 0x06);
        e1000_write_reg(sc.regs(), E1000_IMC1, !0);
        e1000_write_reg(sc.regs(), E1000_IMC2, !0);
    }
    Ok(())
}

/// `em_free_pci_resources`: undoes `em_allocate_pci_resources` and the descriptor rings.
pub fn em_free_pci_resources(sc: &EmSoftc) {
    // Everything below is made after the osdep is written.
    if !sc.osdep_init.load(Ordering::Acquire) {
        return;
    }
    let pc = sc.em_pa().pa_pc;

    if let Some(ih) = NonNull::new(sc.sc_intrhand.replace(ptr::null_mut())) {
        // SAFETY: the handler em_allocate_msix/em_allocate_legacy established; its pointer
        // is gone from the softc.
        unsafe { pci_intr_disestablish(pc, ih) };
    }

    {
        // SAFETY: the detach (or a failed attach): the shared code is not running.
        let o = unsafe { &mut *sc.osdep_ptr() };
        if o.em_flashbase != 0 {
            bus_space_unmap(
                o.flash_bus_space_tag,
                o.flash_bus_space_handle,
                o.em_flashsize,
            );
        }
        o.em_flashbase = 0;

        if o.em_iobase != 0 {
            bus_space_unmap(o.io_bus_space_tag, o.io_bus_space_handle, o.em_iosize);
        }
        o.em_iobase = 0;

        if o.em_membase != 0 {
            bus_space_unmap(o.mem_bus_space_tag, o.mem_bus_space_handle, o.em_memsize);
        }
        o.em_membase = 0;
    }

    for que in sc.queues() {
        if !que.rx.sc_rx_desc_ring.get().is_null() {
            que.rx.sc_rx_desc_ring.set(ptr::null_mut());
            em_dma_free(sc, &que.rx.sc_rx_dma);
        }
        if !que.tx.sc_tx_desc_ring.get().is_null() {
            que.tx.sc_tx_desc_ring.set(ptr::null_mut());
            em_dma_free(sc, &que.tx.sc_tx_dma);
        }
        if let Some(tag) = NonNull::new(que.tag.replace(ptr::null_mut())) {
            // SAFETY: the queue's MSI-X handler, established by em_allocate_msix.
            unsafe { pci_intr_disestablish(pc, tag) };
        }
        que.eims.set(0);
        que.me.set(0);
        que.sc.set(ptr::null());
    }
    sc.legacy_irq.set(0);
    sc.msix_linkvec.set(0);
    sc.msix_queuesmask.set(0);
    if let Some(q) = NonNull::new(sc.queues.get()) {
        free(
            q.cast(),
            M_DEVBUF,
            sc.num_queues.get() as usize * size_of::<EmQueue>(),
        );
    }
    sc.num_queues.set(0);
    sc.queues.set(ptr::null_mut());
}

/// `em_hardware_init`: resets the controller, checks the EEPROM, sets the flow control
/// and runs the shared code's initialization; `EAGAIN` when it is deferred (EP80579).
pub fn em_hardware_init(sc: &EmSoftc) -> Result<(), Errno> {
    let regs = sc.regs();
    let mac_type = sc.mac_type();

    if mac_type >= em_pch_spt {
        em_flush_desc_rings(sc);
    }
    // Issue a global reset
    let _ = em_reset_hw(&mut sc.em_hw());

    // When hardware is reset, fifo_head is also reset
    sc.tx_fifo_head.store(0, Ordering::Relaxed);

    {
        let mut hw = sc.em_hw();

        // Make sure we have a good EEPROM before we read from it
        if em_get_flash_presence_i210(&hw)
            && matches!(em_validate_eeprom_checksum(&mut hw), Err(v) if v < 0)
        {
            // Some PCIe parts fail the first check due to the link being in sleep state,
            // call it again, if it fails a second time its a real issue.
            if matches!(em_validate_eeprom_checksum(&mut hw), Err(v) if v < 0) {
                printf(format_args!(
                    "{}: The EEPROM Checksum Is Not Valid\n",
                    sc.devname()
                ));
                return Err(Errno::EIO);
            }
        }

        if em_get_flash_presence_i210(&hw) {
            let mut part_num = sc.part_num.get();
            let r = em_read_part_num(&mut hw, &mut part_num);
            sc.part_num.set(part_num);
            if matches!(r, Err(v) if v < 0) {
                printf(format_args!(
                    "{}: EEPROM read error while reading part number\n",
                    sc.devname()
                ));
                return Err(Errno::EIO);
            }
        }

        // Set up smart power down as default off on newer adapters
        if !EM_SMART_PWR_DOWN
            && matches!(
                mac_type,
                em_82571 | em_82572 | em_82575 | em_82576 | em_82580 | em_i210 | em_i350
            )
        {
            let mut phy_tmp: u16 = 0;

            // Speed up time to link by disabling smart power down
            let _ = em_read_phy_reg(&mut hw, IGP02E1000_PHY_POWER_MGMT, &mut phy_tmp);
            phy_tmp &= !(IGP02E1000_PM_SPD as u16);
            let _ = em_write_phy_reg(&mut hw, IGP02E1000_PHY_POWER_MGMT, phy_tmp);
        }
    }

    em_legacy_irq_quirk_spt(sc);

    // These parameters control the automatic generation (Tx) and response (Rx) to Ethernet
    // PAUSE frames.
    // - High water mark should allow for at least two frames to be received after sending
    //   an XOFF.
    // - Low water mark works best when it is very near the high water mark. This allows the
    //   receiver to restart by sending XON when it has drained a bit. Here we use an
    //   arbitrary value of 1500 which will restart after one full frame is pulled from the
    //   buffer. There could be several smaller frames in the buffer and if so they will not
    //   trigger the XON until their total number reduces the buffer by 1500.
    // - The pause time is fairly large at 1000 x 512ns = 512 usec.
    let rx_buffer_size = ((e1000_read_reg(regs, E1000_PBA) & 0xffff) << 10) as u16;

    {
        let mut hw = sc.em_hw();
        hw.fc_high_water = rx_buffer_size.wrapping_sub(em_roundup(hw.max_frame_size, 1024) as u16);
        hw.fc_low_water = hw.fc_high_water.wrapping_sub(1500);
        hw.fc_pause_time = if mac_type == em_80003es2lan {
            0xFFFF
        } else {
            1000
        };
        hw.fc_send_xon = true;
        hw.fc = E1000_FC_FULL;
    }

    em_disable_aspm(sc);

    let num_queues = sc.num_queues.get();
    let ret_val = em_init_hw(&mut sc.em_hw(), num_queues);
    if let Err(ret_val) = ret_val {
        if ret_val == E1000_DEFER_INIT {
            return Err(Errno::EAGAIN);
        }
        printf(format_args!(
            "\n{}: Hardware Initialization Failed: {}\n",
            sc.devname(),
            ret_val
        ));
        return Err(Errno::EIO);
    }

    let _ = em_check_for_link(&mut sc.em_hw());

    Ok(())
}

/// The media words `em_setup_interface` adds with `ifmedia_add`, in its order, and how many
/// there are: the fiber ones or the copper ones (without 1000baseT on an IFE PHY), then
/// autoselect.
pub fn em_media_words(
    media_type: EmMediaType,
    mac_type: EmMacType,
    phy_type: EmPhyType,
) -> ([u64; 7], usize) {
    let mut words = [0u64; 7];
    let mut n = 0;
    let mut add = |w: u64| {
        words[n] = w;
        n += 1;
    };
    let mut fiber_type = IFM_1000_SX;

    if media_type == em_media_type_fiber || media_type == em_media_type_internal_serdes {
        if mac_type == em_82545 {
            fiber_type = IFM_1000_LX;
        }
        add(IFM_ETHER | fiber_type | IFM_FDX);
        add(IFM_ETHER | fiber_type);
    } else {
        add(IFM_ETHER | IFM_10_T);
        add(IFM_ETHER | IFM_10_T | IFM_FDX);
        add(IFM_ETHER | IFM_100_TX);
        add(IFM_ETHER | IFM_100_TX | IFM_FDX);
        if phy_type != em_phy_ife {
            add(IFM_ETHER | IFM_1000_T | IFM_FDX);
            add(IFM_ETHER | IFM_1000_T);
        }
    }
    add(IFM_ETHER | IFM_AUTO);
    (words, n)
}

/// `em_setup_interface`: sets up the network interface and attaches it.
pub fn em_setup_interface(sc: &'static EmSoftc) {
    let ifp = sc.ifp();

    let mut xname = [0u8; IFNAMSIZ];
    let name = sc.sc_dev.dv_xname.get();
    let n = name
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(name.len())
        .min(IFNAMSIZ - 1);
    xname[..n].copy_from_slice(&name[..n]);
    ifp.if_xname.set(xname);
    ifp.if_softc.set(ptr::from_ref(sc).cast_mut().cast());
    ifp.if_flags
        .set(IFF_BROADCAST | IFF_SIMPLEX | IFF_MULTICAST);
    ifp.if_xflags.set(IFXF_MPSAFE);
    if sc.sc_dmaflags.get() & BUS_DMA_64BIT != 0 {
        ifp.if_xflags.set(ifp.if_xflags.get() | IFXF_MBUF_64BIT);
    }
    ifp.if_ioctl.set(Some(em_ioctl));
    ifp.if_qstart.set(Some(em_start));
    ifp.if_watchdog.set(Some(em_watchdog));
    let (max_frame_size, media_type, mac_type, phy_type) = {
        let hw = sc.em_hw();
        (hw.max_frame_size, hw.media_type, hw.mac_type, hw.phy_type)
    };
    ifp.if_hardmtu
        .set(max_frame_size - ETHER_HDR_LEN as u32 - ETHER_CRC_LEN as u32);
    ifq_init_maxlen(&ifp.if_snd, sc.sc_tx_slots.get() - 1);

    let mut caps = IFCAP_VLAN_MTU;

    if NVLAN > 0 {
        caps |= IFCAP_VLAN_HWTAGGING;
    }

    if mac_type >= em_82543 {
        caps |= IFCAP_CSUM_TCPv4 | IFCAP_CSUM_UDPv4;
    }
    if mac_type >= em_82575 && mac_type <= em_i210 {
        caps |= IFCAP_CSUM_IPv4;
        caps |= IFCAP_CSUM_TCPv6 | IFCAP_CSUM_UDPv6;
        caps |= IFCAP_TSOv4 | IFCAP_TSOv6;
    }
    ifp.if_capabilities.set(caps);

    // Specify the media types supported by this adapter and register callbacks to update
    // media and link information
    ifmedia_init(&sc.media, IFM_IMASK, em_media_change, em_media_status);
    let (words, nwords) = em_media_words(media_type, mac_type, phy_type);
    for &w in &words[..nwords] {
        ifmedia_add(&sc.media, w, 0, ptr::null_mut());
    }
    ifmedia_set(&sc.media, IFM_ETHER | IFM_AUTO);

    if_attach(ifp);
    ether_ifattach(&sc.sc_ac);
    em_enable_intr(sc);
}

/// `em_detach`.
pub fn em_detach(self_: &Device, _flags: i32) -> Result<(), Errno> {
    let sc = em_sc(self_);
    let ifp = sc.ifp();

    if let Some(ih) = NonNull::new(sc.sc_intrhand.replace(ptr::null_mut())) {
        // SAFETY: the handler em_allocate_msix/em_allocate_legacy established; its pointer
        // is gone from the softc.
        unsafe { pci_intr_disestablish(sc.em_pa().pa_pc, ih) };
    }

    em_stop(sc, true);

    em_free_pci_resources(sc);

    ether_ifdetach(ifp);
    if_detach(ifp);

    Ok(())
}

/// `em_activate`: stops the interface on suspend, starts it again on resume.
pub fn em_activate(self_: &Device, act: i32) -> Result<(), Errno> {
    let sc = em_sc(self_);
    let ifp = sc.ifp();

    match act {
        DVACT_SUSPEND if ifp.if_flags.get() & IFF_RUNNING != 0 => em_stop(sc, false),
        DVACT_RESUME if ifp.if_flags.get() & IFF_UP != 0 => em_init(sc),
        _ => {}
    }
    Ok(())
}

/// `em_smartspeed`: the SmartSpeed workaround of the 82541 and 82547 (IGP PHYs): with no
/// link at gigabit, give up master/slave and then try a 2-pair downshift.
pub fn em_smartspeed(sc: &EmSoftc) {
    let mut hw = sc.em_hw();
    let mut phy_tmp: u16 = 0;

    if sc.link_active.load(Ordering::Relaxed) != 0
        || hw.phy_type != em_phy_igp
        || hw.autoneg == 0
        || hw.autoneg_advertised & ADVERTISE_1000_FULL == 0
    {
        return;
    }

    if sc.smartspeed.get() == 0 {
        // If Master/Slave config fault is asserted twice, we assume back-to-back
        let _ = em_read_phy_reg(&mut hw, PHY_1000T_STATUS, &mut phy_tmp);
        if u32::from(phy_tmp) & SR_1000T_MS_CONFIG_FAULT == 0 {
            return;
        }
        let _ = em_read_phy_reg(&mut hw, PHY_1000T_STATUS, &mut phy_tmp);
        if u32::from(phy_tmp) & SR_1000T_MS_CONFIG_FAULT != 0 {
            let _ = em_read_phy_reg(&mut hw, PHY_1000T_CTRL, &mut phy_tmp);
            if phy_tmp & CR_1000T_MS_ENABLE != 0 {
                phy_tmp &= !CR_1000T_MS_ENABLE;
                let _ = em_write_phy_reg(&mut hw, PHY_1000T_CTRL, phy_tmp);
                sc.smartspeed.set(sc.smartspeed.get() + 1);
                if hw.autoneg != 0
                    && em_phy_setup_autoneg(&mut hw).is_ok()
                    && em_read_phy_reg(&mut hw, PHY_CTRL, &mut phy_tmp).is_ok()
                {
                    phy_tmp |= MII_CR_AUTO_NEG_EN | MII_CR_RESTART_AUTO_NEG;
                    let _ = em_write_phy_reg(&mut hw, PHY_CTRL, phy_tmp);
                }
            }
        }
        return;
    } else if sc.smartspeed.get() == EM_SMARTSPEED_DOWNSHIFT {
        // If still no link, perhaps using 2/3 pair cable
        let _ = em_read_phy_reg(&mut hw, PHY_1000T_CTRL, &mut phy_tmp);
        phy_tmp |= CR_1000T_MS_ENABLE;
        let _ = em_write_phy_reg(&mut hw, PHY_1000T_CTRL, phy_tmp);
        if hw.autoneg != 0
            && em_phy_setup_autoneg(&mut hw).is_ok()
            && em_read_phy_reg(&mut hw, PHY_CTRL, &mut phy_tmp).is_ok()
        {
            phy_tmp |= MII_CR_AUTO_NEG_EN | MII_CR_RESTART_AUTO_NEG;
            let _ = em_write_phy_reg(&mut hw, PHY_CTRL, phy_tmp);
        }
    }
    // Restart process after EM_SMARTSPEED_MAX iterations
    let smartspeed = sc.smartspeed.get();
    sc.smartspeed.set(smartspeed + 1);
    if smartspeed == EM_SMARTSPEED_MAX {
        sc.smartspeed.set(0);
    }
}

// Manage DMA'able memory.

/// `em_dma_malloc`: a descriptor ring of `size` bytes: one segment, mapped and loaded.
pub fn em_dma_malloc(sc: &EmSoftc, size: BusSize, dma: &EmDmaAlloc) -> Result<(), Errno> {
    let dmat = sc.dmat();
    let dmaflags = sc.sc_dmaflags.get();

    let map = bus_dmamap_create(
        dmat,
        size,
        1,
        size,
        0,
        BUS_DMA_WAITOK | BUS_DMA_ALLOCNOW | dmaflags,
    )?;
    dma.dma_map.set(Some(map));
    let destroy = || {
        dma.dma_map.set(None);
        // SAFETY: the map made above, which nothing else holds.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
    };

    let mut seg = [BusDmaSegment::default(); 1];
    let nseg = match bus_dmamem_alloc(
        dmat,
        size,
        PAGE_SIZE,
        0,
        &mut seg,
        BUS_DMA_WAITOK | BUS_DMA_ZERO | dmaflags,
    ) {
        Ok(n) => n,
        Err(r) => {
            // destroy:
            destroy();
            return Err(r);
        }
    };
    dma.dma_seg.set(seg[0]);
    dma.dma_nseg.set(nseg as i32);

    let kva = match bus_dmamem_map(
        dmat,
        &mut seg[..nseg],
        size,
        BUS_DMA_WAITOK | BUS_DMA_COHERENT,
    ) {
        Ok(kva) => kva,
        Err(r) => {
            // free:
            // SAFETY: the segment allocated above, not mapped.
            unsafe { bus_dmamem_free(dmat, &seg[..nseg]) };
            destroy();
            return Err(r);
        }
    };
    dma.dma_vaddr.set(kva.as_ptr());

    // SAFETY: `kva` maps `size` bytes that stay until em_dma_free, which unloads first.
    if let Err(r) = unsafe { bus_dmamap_load(dmat, map, kva.as_ptr(), size, None, BUS_DMA_WAITOK) }
    {
        // unmap:
        // SAFETY: the mapping made above.
        unsafe { bus_dmamem_unmap(dmat, kva, size) };
        // free:
        // SAFETY: the segment allocated above, now unmapped.
        unsafe { bus_dmamem_free(dmat, &seg[..nseg]) };
        destroy();
        return Err(r);
    }

    dma.dma_size.set(size);
    Ok(())
}

/// `em_dma_free`: undoes `em_dma_malloc`.
pub fn em_dma_free(sc: &EmSoftc, dma: &EmDmaAlloc) {
    let dmat = sc.dmat();
    let map = dma.map();
    let seg = [dma.dma_seg.get()];
    let nseg = (dma.dma_nseg.get().max(0) as usize).min(seg.len());

    bus_dmamap_unload(dmat, map);
    if let Some(kva) = NonNull::new(dma.dma_vaddr.replace(ptr::null_mut())) {
        // SAFETY: em_dma_malloc's mapping, unloaded above; the ring pointer is cleared by
        // the caller.
        unsafe { bus_dmamem_unmap(dmat, kva, dma.dma_size.get()) };
    }
    // SAFETY: em_dma_malloc's segment, now unmapped.
    unsafe { bus_dmamem_free(dmat, &seg[..nseg]) };
    dma.dma_map.set(None);
    // SAFETY: em_dma_malloc's map, unloaded and no longer held.
    unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
}

/// `em_allocate_transmit_structures`: the transmit packet slots.
pub fn em_allocate_transmit_structures(sc: &EmSoftc) -> Result<(), Errno> {
    for que in sc.queues() {
        let ring = que.tx.sc_tx_dma.map();
        bus_dmamap_sync(
            sc.dmat(),
            ring,
            0,
            ring.dm_mapsize.get(),
            BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
        );

        let Some(p) = mallocarray(
            sc.sc_tx_slots.get() as usize,
            size_of::<EmPacket>(),
            M_DEVBUF,
            M_NOWAIT | M_ZERO,
        ) else {
            printf(format_args!(
                "{}: Unable to allocate tx_buffer memory\n",
                sc.devname()
            ));
            return Err(Errno::ENOMEM);
        };
        que.tx.sc_tx_pkts_ring.set(p.as_ptr().cast());
    }

    Ok(())
}

/// `em_setup_transmit_structures`: allocates and initializes the transmit structures.
pub fn em_setup_transmit_structures(sc: &EmSoftc) -> Result<(), Errno> {
    let r = 'fail: {
        if let Err(e) = em_allocate_transmit_structures(sc) {
            break 'fail Err(e);
        }

        let slots = sc.sc_tx_slots.get();
        let nsegments = EM_MAX_SCATTER / if sc.pcix_82544.get() { 2 } else { 1 };
        for que in sc.queues() {
            // SAFETY: the ring maps `sc_tx_slots` descriptors (`txd` checks the base); the
            // transmitter is stopped (em_init ran em_stop).
            unsafe { ptr::write_bytes(que.txd(0), 0, slots as usize) };

            for i in 0..slots {
                let pkt = que.tx_pkt(i);
                match bus_dmamap_create(
                    sc.dmat(),
                    EM_TSO_SIZE,
                    nsegments as i32,
                    EM_TSO_SEG_SIZE,
                    0,
                    BUS_DMA_NOWAIT | sc.sc_dmaflags.get(),
                ) {
                    Ok(map) => pkt.pkt_map.set(Some(map)),
                    Err(error) => {
                        printf(format_args!(
                            "{}: Unable to create TX DMA map, error {}\n",
                            sc.devname(),
                            error as i32
                        ));
                        break 'fail Err(error);
                    }
                }
            }

            que.tx.sc_tx_desc_head.store(0, Ordering::Relaxed);
            que.tx.sc_tx_desc_tail.store(0, Ordering::Relaxed);

            // Set checksum context
            que.tx.active_checksum_context.set(OFFLOAD_NONE);
        }

        Ok(())
    };

    if r.is_err() {
        // fail:
        em_free_transmit_structures(sc);
    }
    r
}

/// `em_initialize_transmit_unit`: enables the transmit unit.
pub fn em_initialize_transmit_unit(sc: &EmSoftc) {
    let regs = sc.regs();
    let mac_type = sc.mac_type();
    let media_type = sc.em_hw().media_type;
    let slots = sc.sc_tx_slots.get();

    for que in sc.queues() {
        let me = que.me.get();

        // Setup the Base and Length of the Tx Descriptor Ring
        let bus_addr = que.tx.sc_tx_dma.map().dm_segs()[0].get().ds_addr as u64;
        e1000_write_reg(regs, e1000_tdlen(me), slots * size_of::<EmTxDesc>() as u32);
        e1000_write_reg(regs, e1000_tdbah(me), (bus_addr >> 32) as u32);
        e1000_write_reg(regs, e1000_tdbal(me), bus_addr as u32);

        // Setup the HW Tx Head and Tail descriptor pointers
        e1000_write_reg(regs, e1000_tdt(me), 0);
        e1000_write_reg(regs, e1000_tdh(me), 0);

        // Set the default values for the Tx Inter Packet Gap timer
        let reg_tipg = match mac_type {
            em_82542_rev2_0 | em_82542_rev2_1 => {
                DEFAULT_82542_TIPG_IPGT
                    | (DEFAULT_82542_TIPG_IPGR1 << E1000_TIPG_IPGR1_SHIFT)
                    | (DEFAULT_82542_TIPG_IPGR2 << E1000_TIPG_IPGR2_SHIFT)
            }
            em_80003es2lan => {
                DEFAULT_82543_TIPG_IPGR1
                    | (DEFAULT_80003ES2LAN_TIPG_IPGR2 << E1000_TIPG_IPGR2_SHIFT)
            }
            _ => {
                let ipgt = if media_type == em_media_type_fiber
                    || media_type == em_media_type_internal_serdes
                {
                    DEFAULT_82543_TIPG_IPGT_FIBER
                } else {
                    DEFAULT_82543_TIPG_IPGT_COPPER
                };
                ipgt | (DEFAULT_82543_TIPG_IPGR1 << E1000_TIPG_IPGR1_SHIFT)
                    | (DEFAULT_82543_TIPG_IPGR2 << E1000_TIPG_IPGR2_SHIFT)
            }
        };

        e1000_write_reg(regs, E1000_TIPG, reg_tipg);
        e1000_write_reg(regs, E1000_TIDV, sc.tx_int_delay.get());
        if mac_type >= em_82540 {
            e1000_write_reg(regs, E1000_TADV, sc.tx_abs_int_delay.get());
        }

        // Setup Transmit Descriptor Base Settings
        que.tx.sc_txd_cmd.set(E1000_TXD_CMD_IFCS);

        if matches!(mac_type, em_82575 | em_82580 | em_82576 | em_i210 | em_i350) {
            // 82575/6 need to enable the TX queue and lack the IDE bit
            let reg_tctl = e1000_read_reg(regs, e1000_txdctl(me)) | E1000_TXDCTL_QUEUE_ENABLE;
            e1000_write_reg(regs, e1000_txdctl(me), reg_tctl);
        } else if sc.tx_int_delay.get() > 0 {
            que.tx
                .sc_txd_cmd
                .set(que.tx.sc_txd_cmd.get() | E1000_TXD_CMD_IDE);
        }
    }

    // Program the Transmit Control Register
    let mut reg_tctl =
        E1000_TCTL_PSP | E1000_TCTL_EN | (E1000_COLLISION_THRESHOLD << E1000_CT_SHIFT);
    if mac_type >= em_82571 {
        reg_tctl |= E1000_TCTL_MULR;
    }
    if sc.link_duplex.load(Ordering::Relaxed) == FULL_DUPLEX {
        reg_tctl |= E1000_FDX_COLLISION_DISTANCE << E1000_COLD_SHIFT;
    } else {
        reg_tctl |= E1000_HDX_COLLISION_DISTANCE << E1000_COLD_SHIFT;
    }
    // This write will effectively turn on the transmit unit
    e1000_write_reg(regs, E1000_TCTL, reg_tctl);

    // SPT Si errata workaround to avoid data corruption

    if mac_type == em_pch_spt {
        let reg_val = em_read_reg(regs, E1000_IOSFPC) | E1000_RCTL_RDMTS_HEX;
        em_write_reg(regs, E1000_IOSFPC, reg_val);

        let mut reg_val = e1000_read_reg(regs, E1000_TARC0);
        // i218-i219 Specification Update 1.5.4.5
        reg_val &= !E1000_TARC0_CB_MULTIQ_3_REQ;
        reg_val |= E1000_TARC0_CB_MULTIQ_2_REQ;
        e1000_write_reg(regs, E1000_TARC0, reg_val);
    }
}

/// `em_free_transmit_structures`: frees the transmit packets, their maps and slots.
pub fn em_free_transmit_structures(sc: &EmSoftc) {
    let dmat = sc.dmat();
    let slots = sc.sc_tx_slots.get();

    for que in sc.queues() {
        let ring = que.tx.sc_tx_pkts_ring.get();
        if !ring.is_null() {
            for i in 0..slots {
                let pkt = que.tx_pkt(i);

                if let Some(m) = pkt.pkt_m.take() {
                    let map = pkt.map();
                    bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTWRITE);
                    bus_dmamap_unload(dmat, map);

                    m_freem(m);
                }

                if let Some(map) = pkt.pkt_map.take() {
                    // SAFETY: the slot's map, unloaded, which no one else holds.
                    unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
                }
            }

            que.tx.sc_tx_pkts_ring.set(ptr::null_mut());
            if let Some(p) = NonNull::new(ring) {
                free(p.cast(), M_DEVBUF, slots as usize * size_of::<EmPacket>());
            }
        }

        let map = que.tx.sc_tx_dma.map();
        bus_dmamap_sync(
            dmat,
            map,
            0,
            map.dm_mapsize.get(),
            BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
        );
    }
}

/// Writes an advanced context descriptor into transmit slot `head`.
fn em_write_adv_ctx(que: &EmQueue, head: u32, d: E1000AdvTxContextDesc) {
    let td = que.txd(head).cast::<E1000AdvTxContextDesc>();
    // SAFETY: a slot of the mapped ring (`txd`); the context descriptor has the size and
    // alignment of a transmit descriptor (asserted below), as the C's cast relies on.
    unsafe { ptr::write_volatile(td, d) };
}

/// `em_tso_setup`: the advanced context descriptor of a TSO packet in slot `head`; the
/// slots used (1), or 0 when the packet cannot be segmented.
pub fn em_tso_setup(
    que: &EmQueue,
    mp: &Mbuf,
    head: u32,
    olinfo_status: &mut u32,
    cmd_type_len: &mut u32,
) -> u32 {
    let mut ext = EtherExtracted::new();
    let mut vlan_macip_lens: u32 = 0;
    let mut type_tucmd_mlhl: u32 = 0;
    let mut mss_l4len_idx: u32 = 0;

    *olinfo_status = 0;
    *cmd_type_len = 0;

    if NVLAN > 0 && mp.m_flags().get() & M_VLANTAG != 0 {
        let vtag = u32::from(mp.m_pkthdr().ether_vtag.get());
        vlan_macip_lens |= vtag << E1000_ADVTXD_VLAN_SHIFT;
        *cmd_type_len |= E1000_ADVTXD_DCMD_VLE;
    }

    ether_extract_headers(mp, &mut ext);
    let ph_mss = u32::from(mp.m_pkthdr().ph_mss.get());
    let ok = 'out: {
        if ext.tcp.is_null() || ph_mss == 0 {
            break 'out false;
        }

        vlan_macip_lens |= (ETHER_HDR_LEN as u32) << E1000_ADVTXD_MACLEN_SHIFT;

        if !ext.ip4.is_null() {
            type_tucmd_mlhl |= E1000_ADVTXD_TUCMD_IPV4;
            *olinfo_status |= E1000_TXD_POPTS_IXSM << 8;
        } else if cfg!(feature = "inet6") && !ext.ip6.is_null() {
            type_tucmd_mlhl |= E1000_ADVTXD_TUCMD_IPV6;
        } else {
            break 'out false;
        }

        true
    };
    if !ok {
        // out:
        tcpstat_inc(TcpstatCounters::TcpsOutbadtso);
        return 0;
    }

    *cmd_type_len |= E1000_ADVTXD_DTYP_DATA | E1000_ADVTXD_DCMD_IFCS;
    *cmd_type_len |= E1000_ADVTXD_DCMD_DEXT | E1000_ADVTXD_DCMD_TSE;
    *olinfo_status |= ext.paylen << E1000_ADVTXD_PAYLEN_SHIFT;
    vlan_macip_lens |= ext.iphlen;
    type_tucmd_mlhl |= E1000_ADVTXD_DCMD_DEXT | E1000_ADVTXD_DTYP_CTXT;

    type_tucmd_mlhl |= E1000_ADVTXD_TUCMD_L4T_TCP;
    *olinfo_status |= E1000_TXD_POPTS_TXSM << 8;

    mss_l4len_idx |= ph_mss << E1000_ADVTXD_MSS_SHIFT;
    mss_l4len_idx |= ext.tcphlen << E1000_ADVTXD_L4LEN_SHIFT;
    // 82575 needs the queue index added
    if que.sc().mac_type() == em_82575 {
        mss_l4len_idx |= (que.me.get() & 0xff) << 4;
    }

    em_write_adv_ctx(
        que,
        head,
        E1000AdvTxContextDesc {
            vlan_macip_lens: vlan_macip_lens.to_le(),
            u: E1000AdvTxContextDescU { seqnum_seed: 0 },
            type_tucmd_mlhl: type_tucmd_mlhl.to_le(),
            mss_l4len_idx: mss_l4len_idx.to_le(),
        },
    );

    tcpstat_add(
        TcpstatCounters::TcpsOutpkttso,
        u64::from(ext.paylen.div_ceil(ph_mss)),
    );

    1
}

/// `em_tx_ctx_setup`: the advanced context descriptor of a packet with checksum offload in
/// slot `head`; the slots used (0 or 1).
pub fn em_tx_ctx_setup(
    que: &EmQueue,
    mp: &Mbuf,
    head: u32,
    olinfo_status: &mut u32,
    cmd_type_len: &mut u32,
) -> u32 {
    let mut ext = EtherExtracted::new();
    let mut vlan_macip_lens: u32 = 0;
    let mut type_tucmd_mlhl: u32 = 0;
    let mut mss_l4len_idx: u32 = 0;
    let mut off = false;
    let csum_flags = mp.m_pkthdr().csum_flags.get();

    *olinfo_status = 0;
    *cmd_type_len = 0;

    if NVLAN > 0 && mp.m_flags().get() & M_VLANTAG != 0 {
        let vtag = u32::from(mp.m_pkthdr().ether_vtag.get());
        vlan_macip_lens |= vtag << E1000_ADVTXD_VLAN_SHIFT;
        *cmd_type_len |= E1000_ADVTXD_DCMD_VLE;
        off = true;
    }

    ether_extract_headers(mp, &mut ext);

    vlan_macip_lens |= (ETHER_HDR_LEN as u32) << E1000_ADVTXD_MACLEN_SHIFT;

    if !ext.ip4.is_null() {
        type_tucmd_mlhl |= E1000_ADVTXD_TUCMD_IPV4;
        if csum_flags & M_IPV4_CSUM_OUT != 0 {
            *olinfo_status |= E1000_TXD_POPTS_IXSM << 8;
            off = true;
        }
    } else if cfg!(feature = "inet6") && !ext.ip6.is_null() {
        type_tucmd_mlhl |= E1000_ADVTXD_TUCMD_IPV6;
    }

    *cmd_type_len |= E1000_ADVTXD_DTYP_DATA | E1000_ADVTXD_DCMD_IFCS;
    *cmd_type_len |= E1000_ADVTXD_DCMD_DEXT;
    *olinfo_status |= (mp.m_pkthdr().len.get() as u32) << E1000_ADVTXD_PAYLEN_SHIFT;
    vlan_macip_lens |= ext.iphlen;
    type_tucmd_mlhl |= E1000_ADVTXD_DCMD_DEXT | E1000_ADVTXD_DTYP_CTXT;

    if !ext.tcp.is_null() {
        type_tucmd_mlhl |= E1000_ADVTXD_TUCMD_L4T_TCP;
        if csum_flags & M_TCP_CSUM_OUT != 0 {
            *olinfo_status |= E1000_TXD_POPTS_TXSM << 8;
            off = true;
        }
    } else if !ext.udp.is_null() {
        type_tucmd_mlhl |= E1000_ADVTXD_TUCMD_L4T_UDP;
        if csum_flags & M_UDP_CSUM_OUT != 0 {
            *olinfo_status |= E1000_TXD_POPTS_TXSM << 8;
            off = true;
        }
    }

    if !off {
        return 0;
    }

    // 82575 needs the queue index added
    if que.sc().mac_type() == em_82575 {
        mss_l4len_idx |= (que.me.get() & 0xff) << 4;
    }

    em_write_adv_ctx(
        que,
        head,
        E1000AdvTxContextDesc {
            vlan_macip_lens: vlan_macip_lens.to_le(),
            u: E1000AdvTxContextDescU { seqnum_seed: 0 },
            type_tucmd_mlhl: type_tucmd_mlhl.to_le(),
            mss_l4len_idx: mss_l4len_idx.to_le(),
        },
    );

    1
}

/// `em_transmit_checksum_setup`: the offload context is loaded with the first packet of a
/// protocol (TCP/UDP) and changed only when the protocol changes; the slots used (0 or 1).
pub fn em_transmit_checksum_setup(
    que: &EmQueue,
    mp: &Mbuf,
    head: u32,
    txd_upper: &mut u32,
    txd_lower: &mut u32,
) -> u32 {
    let csum_flags = mp.m_pkthdr().csum_flags.get();

    if csum_flags & M_TCP_CSUM_OUT != 0 {
        *txd_upper = E1000_TXD_POPTS_TXSM << 8;
        *txd_lower = E1000_TXD_CMD_DEXT | E1000_TXD_DTYP_D;
        if que.tx.active_checksum_context.get() == OFFLOAD_TCP_IP {
            return 0;
        } else {
            que.tx.active_checksum_context.set(OFFLOAD_TCP_IP);
        }
    } else if csum_flags & M_UDP_CSUM_OUT != 0 {
        *txd_upper = E1000_TXD_POPTS_TXSM << 8;
        *txd_lower = E1000_TXD_CMD_DEXT | E1000_TXD_DTYP_D;
        if que.tx.active_checksum_context.get() == OFFLOAD_UDP_IP {
            return 0;
        } else {
            que.tx.active_checksum_context.set(OFFLOAD_UDP_IP);
        }
    } else {
        *txd_upper = 0;
        *txd_lower = 0;
        return 0;
    }

    // If we reach this point, the checksum offload context needs to be reset.
    let ip_hlen = size_of::<Ip>();
    let tucso = if que.tx.active_checksum_context.get() == OFFLOAD_TCP_IP {
        ETHER_HDR_LEN + ip_hlen + TCPHDR_TH_SUM
    } else {
        // OFFLOAD_UDP_IP
        ETHER_HDR_LEN + ip_hlen + UDPHDR_UH_SUM
    };
    let txd = EmContextDesc {
        lower_setup: EmContextDescLowerSetup {
            ip_fields: EmContextDescIpFields {
                ipcss: ETHER_HDR_LEN as u8,
                ipcso: (ETHER_HDR_LEN + offset_of!(Ip, ip_sum)) as u8,
                ipcse: ((ETHER_HDR_LEN + ip_hlen - 1) as u16).to_le(),
            },
        },
        upper_setup: EmContextDescUpperSetup {
            tcp_fields: EmContextDescTcpFields {
                tucss: (ETHER_HDR_LEN + ip_hlen) as u8,
                tucso: tucso as u8,
                tucse: 0u16.to_le(),
            },
        },
        cmd_and_length: (que.tx.sc_txd_cmd.get() | E1000_TXD_CMD_DEXT).to_le(),
        tcp_seg_setup: EmContextDescTcpSegSetup { data: 0u32.to_le() },
    };
    let p = que.txd(head).cast::<EmContextDesc>();
    // SAFETY: a slot of the mapped ring (`txd`); the context descriptor has the size and
    // alignment of a transmit descriptor (asserted below), as the C's cast relies on.
    unsafe { ptr::write_volatile(p, txd) };

    1
}

/// `em_txeof`: reclaims the transmit descriptors the controller is done with, frees their
/// packets and restarts the send queue.
pub fn em_txeof(que: &EmQueue) {
    let sc = que.sc();
    let ifp = sc.ifp();
    let dmat = sc.dmat();
    let slots = sc.sc_tx_slots.get();
    let mut free: u32 = 0;

    let head = que.tx.sc_tx_desc_head.load(Ordering::Acquire);
    let mut tail = que.tx.sc_tx_desc_tail.load(Ordering::Relaxed);

    if head == tail {
        return;
    }

    let ring = que.tx.sc_tx_dma.map();
    bus_dmamap_sync(dmat, ring, 0, ring.dm_mapsize.get(), BUS_DMASYNC_POSTREAD);

    loop {
        let pkt = que.tx_pkt(tail);
        let desc = que.txd_get(pkt.pkt_eop.get());

        if u32::from(desc.upper.fields().status) & E1000_TXD_STAT_DD == 0 {
            break;
        }

        let map = pkt.map();
        bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTWRITE);
        bus_dmamap_unload(dmat, map);

        let m = pkt.pkt_m.take();
        kassert!(m.is_some());

        m_freem(m);

        tail = pkt.pkt_eop.get();

        tail += 1;
        if tail == slots {
            tail = 0;
        }

        free += 1;
        if tail == head {
            break;
        }
    }

    bus_dmamap_sync(dmat, ring, 0, ring.dm_mapsize.get(), BUS_DMASYNC_PREREAD);

    if free == 0 {
        return;
    }

    que.tx.sc_tx_desc_tail.store(tail, Ordering::Release);

    if ifq_is_oactive(&ifp.if_snd) {
        ifq_restart(&ifp.if_snd);
    } else if tail == head {
        ifp.if_timer.set(0);
    }
}

/// `em_get_buf`: a cluster for receive slot `i`, loaded and handed to the descriptor.
pub fn em_get_buf(que: &EmQueue, i: u32) -> Result<(), Errno> {
    let sc = que.sc();
    let dmat = sc.dmat();
    let pkt = que.rx_pkt(i);

    kassert!(pkt.pkt_m.get().is_none());

    let Some(m) = m_clget(None, M_DONTWAIT, EM_MCLBYTES) else {
        sc.mbuf_cluster_failed.set(sc.mbuf_cluster_failed.get() + 1);
        return Err(Errno::ENOBUFS);
    };
    m.m_len().set(EM_MCLBYTES);
    m.m_pkthdr().len.set(EM_MCLBYTES as i32);
    m_adj(m, ETHER_ALIGN as i32);

    let map = pkt.map();
    // SAFETY: the cluster stays the slot's (`pkt_m`) until em_rxeof or
    // em_free_receive_structures unload the map before handing it on or freeing it.
    if let Err(error) = unsafe { bus_dmamap_load_mbuf(dmat, map, m, BUS_DMA_NOWAIT) } {
        m_freem(m);
        return Err(error);
    }

    bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_PREREAD);
    pkt.pkt_m.set(Some(m));

    que.rxd_set(
        i,
        EmRxDesc {
            buffer_addr: (map.dm_segs()[0].get().ds_addr as u64).to_le(),
            ..EmRxDesc::default()
        },
    );

    Ok(())
}

/// `em_allocate_receive_structures`: the receive packet slots and their maps (one per
/// descriptor).
pub fn em_allocate_receive_structures(sc: &EmSoftc) -> Result<(), Errno> {
    let slots = sc.sc_rx_slots.get();

    let r = 'fail: {
        for que in sc.queues() {
            let Some(p) = mallocarray(
                slots as usize,
                size_of::<EmPacket>(),
                M_DEVBUF,
                M_NOWAIT | M_ZERO,
            ) else {
                printf(format_args!(
                    "{}: Unable to allocate rx_buffer memory\n",
                    sc.devname()
                ));
                return Err(Errno::ENOMEM);
            };
            que.rx.sc_rx_pkts_ring.set(p.as_ptr().cast());

            let ring = que.rx.sc_rx_dma.map();
            bus_dmamap_sync(
                sc.dmat(),
                ring,
                0,
                ring.dm_mapsize.get(),
                BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE,
            );

            for i in 0..slots {
                let pkt = que.rx_pkt(i);

                match bus_dmamap_create(
                    sc.dmat(),
                    EM_MCLBYTES as BusSize,
                    1,
                    EM_MCLBYTES as BusSize,
                    0,
                    BUS_DMA_NOWAIT | sc.sc_dmaflags.get(),
                ) {
                    Ok(map) => pkt.pkt_map.set(Some(map)),
                    Err(error) => {
                        printf(format_args!(
                            "{}: Unable to create RX DMA map, error {}\n",
                            sc.devname(),
                            error as i32
                        ));
                        break 'fail Err(error);
                    }
                }

                pkt.pkt_m.set(None);
            }
        }

        Ok(())
    };

    if r.is_err() {
        // fail:
        em_free_receive_structures(sc);
    }
    r
}

/// `em_setup_receive_structures`: allocates and initializes the receive structures.
pub fn em_setup_receive_structures(sc: &'static EmSoftc) -> Result<(), Errno> {
    let ifp = sc.ifp();
    let slots = sc.sc_rx_slots.get();

    if em_allocate_receive_structures(sc).is_err() {
        return Err(Errno::ENOMEM);
    }

    for que in sc.queues() {
        // SAFETY: the ring maps `sc_rx_slots` descriptors (`rxd` checks the base); the
        // receiver is stopped (em_init ran em_stop).
        unsafe { ptr::write_bytes(que.rxd(0), 0, slots as usize) };

        // Setup our descriptor pointers
        que.rx.sc_rx_desc_tail.set(0);
        que.rx.sc_rx_desc_head.set(slots - 1);

        let lwm = 4.max(2 * ((ifp.if_hardmtu.get() / MCLBYTES as u32) + 1));
        que.with_rx_ring(|r| if_rxr_init(r, lwm, slots));

        if !em_rxfill(que) {
            printf(format_args!(
                "{}: unable to fill any rx descriptors\n",
                sc.devname()
            ));
            return Err(Errno::ENOMEM);
        }
    }

    Ok(())
}

/// `em_initialize_receive_unit`: enables the receive unit.
pub fn em_initialize_receive_unit(sc: &EmSoftc) {
    let regs = sc.regs();
    let mac_type = sc.mac_type();
    let slots = sc.sc_rx_slots.get();
    let (mc_filter_type, tbi_compatibility_on, max_frame_size) = {
        let hw = sc.em_hw();
        (
            hw.mc_filter_type,
            hw.tbi_compatibility_on,
            hw.max_frame_size,
        )
    };

    // Make sure receives are disabled while setting up the descriptor ring
    e1000_write_reg(regs, E1000_RCTL, 0);

    // Set the Receive Delay Timer Register
    e1000_write_reg(regs, E1000_RDTR, sc.rx_int_delay.get() | E1000_RDT_FPDB);

    if mac_type >= em_82540 {
        if sc.rx_int_delay.get() != 0 {
            e1000_write_reg(regs, E1000_RADV, sc.rx_abs_int_delay.get());
        }

        // Set the interrupt throttling rate. Value is calculated as DEFAULT_ITR =
        // 1/(MAX_INTS_PER_SEC * 256ns)
        e1000_write_reg(regs, E1000_ITR, DEFAULT_ITR);
    }

    // Setup the Receive Control Register
    let mut reg_rctl = E1000_RCTL_EN
        | E1000_RCTL_BAM
        | E1000_RCTL_LBM_NO
        | E1000_RCTL_RDMTS_HALF
        | (mc_filter_type << E1000_RCTL_MO_SHIFT);

    if tbi_compatibility_on {
        reg_rctl |= E1000_RCTL_SBP;
    }

    // The i350 has a bug where it always strips the CRC whether asked to or not. So ask
    // for stripped CRC here and cope in rxeof
    if mac_type == em_i210 || mac_type == em_i350 {
        reg_rctl |= E1000_RCTL_SECRC;
    }

    reg_rctl |= match sc.sc_rx_buffer_len.get() {
        EM_RXBUFFER_4096 => E1000_RCTL_SZ_4096 | E1000_RCTL_BSEX | E1000_RCTL_LPE,
        EM_RXBUFFER_8192 => E1000_RCTL_SZ_8192 | E1000_RCTL_BSEX | E1000_RCTL_LPE,
        EM_RXBUFFER_16384 => E1000_RCTL_SZ_16384 | E1000_RCTL_BSEX | E1000_RCTL_LPE,
        // default, EM_RXBUFFER_2048
        _ => E1000_RCTL_SZ_2048,
    };

    if max_frame_size != ETHER_MAX_LEN as u32 {
        reg_rctl |= E1000_RCTL_LPE;
    }

    // Enable 82543 Receive Checksum Offload for TCP and UDP
    if mac_type >= em_82543 {
        let reg_rxcsum =
            e1000_read_reg(regs, E1000_RXCSUM) | E1000_RXCSUM_IPOFL | E1000_RXCSUM_TUOFL;
        e1000_write_reg(regs, E1000_RXCSUM, reg_rxcsum);
    }

    // XXX TEMPORARY WORKAROUND: on some systems with 82573 long latencies are observed,
    // like Lenovo X60.
    if mac_type == em_82573 {
        e1000_write_reg(regs, E1000_RDTR, 0x20);
    }

    for que in sc.queues() {
        let me = que.me.get();
        if sc.num_queues.get() > 1 {
            // Disable Drop Enable for every queue, default has it enabled for queues > 0
            let reg_srrctl = e1000_read_reg(regs, e1000_srrctl(me)) & !E1000_SRRCTL_DROP_EN;
            e1000_write_reg(regs, e1000_srrctl(me), reg_srrctl);
        }

        // Setup the Base and Length of the Rx Descriptor Ring
        let bus_addr = que.rx.sc_rx_dma.map().dm_segs()[0].get().ds_addr as u64;
        e1000_write_reg(regs, e1000_rdlen(me), slots * size_of::<EmRxDesc>() as u32);
        e1000_write_reg(regs, e1000_rdbah(me), (bus_addr >> 32) as u32);
        e1000_write_reg(regs, e1000_rdbal(me), bus_addr as u32);

        if matches!(mac_type, em_82575 | em_82580 | em_82576 | em_i210 | em_i350) {
            // 82575/6 need to enable the RX queue
            let reg = e1000_read_reg(regs, e1000_rxdctl(me)) | E1000_RXDCTL_QUEUE_ENABLE;
            e1000_write_reg(regs, e1000_rxdctl(me), reg);
        }
    }

    // Enable Receives
    e1000_write_reg(regs, E1000_RCTL, reg_rctl);

    // Setup the HW Rx Head and Tail Descriptor Pointers
    for que in sc.queues() {
        e1000_write_reg(regs, e1000_rdh(que.me.get()), 0);
        e1000_write_reg(regs, e1000_rdt(que.me.get()), que.rx.sc_rx_desc_head.get());
    }
}

/// `em_free_receive_structures`: frees the receive packets, their maps and slots.
pub fn em_free_receive_structures(sc: &EmSoftc) {
    let dmat = sc.dmat();
    let slots = sc.sc_rx_slots.get();

    for que in sc.queues() {
        que.with_rx_ring(|r| if_rxr_init(r, 0, 0));

        let ring = que.rx.sc_rx_dma.map();
        bus_dmamap_sync(
            dmat,
            ring,
            0,
            ring.dm_mapsize.get(),
            BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
        );

        let pkts = que.rx.sc_rx_pkts_ring.get();
        if !pkts.is_null() {
            for i in 0..slots {
                let pkt = que.rx_pkt(i);
                if let Some(m) = pkt.pkt_m.take() {
                    let map = pkt.map();
                    bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTREAD);
                    bus_dmamap_unload(dmat, map);
                    m_freem(m);
                }
                if let Some(map) = pkt.pkt_map.take() {
                    // SAFETY: the slot's map, unloaded, which no one else holds.
                    unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
                }
            }

            que.rx.sc_rx_pkts_ring.set(ptr::null_mut());
            if let Some(p) = NonNull::new(pkts) {
                free(p.cast(), M_DEVBUF, slots as usize * size_of::<EmPacket>());
            }
        }

        if let Some(fmp) = que.rx.fmp.take() {
            m_freem(fmp);
            que.rx.lmp.set(None);
        }
    }
}

/// `em_rxfill`: gives the receive ring as many new clusters as it may take; whether any
/// was given.
pub fn em_rxfill(que: &EmQueue) -> bool {
    let sc = que.sc();
    let dmat = sc.dmat();
    let slots = sc.sc_rx_slots.get();
    let mut post = false;

    let mut i = que.rx.sc_rx_desc_head.get();

    let ring = que.rx.sc_rx_dma.map();
    bus_dmamap_sync(dmat, ring, 0, ring.dm_mapsize.get(), BUS_DMASYNC_POSTWRITE);

    let mut n = que.with_rx_ring(|r| if_rxr_get(r, slots));
    while n > 0 {
        i += 1;
        if i == slots {
            i = 0;
        }

        if em_get_buf(que, i).is_err() {
            break;
        }

        que.rx.sc_rx_desc_head.set(i);
        post = true;
        n -= 1;
    }

    que.with_rx_ring(|r| if_rxr_put(r, n));

    bus_dmamap_sync(dmat, ring, 0, ring.dm_mapsize.get(), BUS_DMASYNC_PREWRITE);

    post
}

/// `em_rxrefill`: `rx_refill`'s function.
pub fn em_rxrefill(arg: *mut c_void) {
    // SAFETY: em_allocate_pci_resources set the timeout with the queue as its argument; the
    // queue is freed only after em_stop deleted the timeout.
    let que = unsafe { &*arg.cast::<EmQueue>().cast_const() };

    let s = splnet();
    em_rxrefill_locked(que);
    splx(s);
}

/// `em_rxrefill_locked`: refills the receive ring and moves its tail register, or tries again
/// in a tick when the ring is empty.
pub fn em_rxrefill_locked(que: &EmQueue) {
    let sc = que.sc();

    if em_rxfill(que) {
        e1000_write_reg(
            sc.regs(),
            e1000_rdt(que.me.get()),
            que.rx.sc_rx_desc_head.get(),
        );
    } else if if_rxr_needrefill(&que.rx.sc_rx_ring.get()) {
        timeout_add(&que.rx_refill, 1);
    }
}

/// `em_rxeof`: in interrupt context, takes the packets the controller received off the ring
/// and passes them up; whether any descriptor was done.
pub fn em_rxeof(que: &EmQueue) -> bool {
    let sc = que.sc();
    let ifp = sc.ifp();
    let dmat = sc.dmat();
    let mac_type = sc.mac_type();
    let slots = sc.sc_rx_slots.get();
    let ml = MbufList::new();
    let mut rv = false;

    if if_rxr_inuse(&que.rx.sc_rx_ring.get()) == 0 {
        return false;
    }

    let mut i = que.rx.sc_rx_desc_tail.get();

    let ring = que.rx.sc_rx_dma.map();
    bus_dmamap_sync(dmat, ring, 0, ring.dm_mapsize.get(), BUS_DMASYNC_POSTREAD);

    loop {
        let pkt = que.rx_pkt(i);

        let status = que.rxd_status(i);
        if u32::from(status) & E1000_RXD_STAT_DD == 0 {
            break;
        }
        let desc = que.rxd_get(i);

        // pull the mbuf off the ring
        let map = pkt.map();
        bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTREAD);
        bus_dmamap_unload(dmat, map);
        let Some(m) = pkt.pkt_m.take() else {
            panic(format_args!("{}: no mbuf in rx slot {i}", sc.devname()));
        };

        que.with_rx_ring(|r| if_rxr_put(r, 1));
        rv = true;

        let mut accept_frame = true;
        let mut prev_len_adj: u16 = 0;
        let desc_len = u16::from_le(desc.length);
        let eop;
        let mut len: u16;

        if u32::from(status) & E1000_RXD_STAT_EOP != 0 {
            eop = true;
            if mac_type == em_i210 || mac_type == em_i350 {
                // crc has already been stripped
                len = desc_len;
            } else if usize::from(desc_len) < ETHER_CRC_LEN {
                len = 0;
                prev_len_adj = ETHER_CRC_LEN as u16 - desc_len;
            } else {
                len = desc_len - ETHER_CRC_LEN as u16;
            }
        } else {
            eop = false;
            len = desc_len;
        }

        if u32::from(desc.errors) & E1000_RXD_ERR_FRAME_ERR_MASK != 0 {
            let mut pkt_len = u32::from(desc_len);

            if let Some(fmp) = que.rx.fmp.get() {
                pkt_len += fmp.m_pkthdr().len.get() as u32;
            }

            let last_byte = if desc_len > 0 && u32::from(desc_len) <= m.m_len().get() {
                // SAFETY: the controller wrote `desc_len` bytes at the cluster's data, which
                // holds `m_len` bytes; the last one is inside it.
                unsafe { ptr::read_volatile(mtod::<u8>(m).add(usize::from(desc_len) - 1)) }
            } else {
                0
            };
            if tbi_accept_with(
                sc.tbi_compatibility_on.load(Ordering::Relaxed),
                sc.min_frame_size.load(Ordering::Relaxed),
                sc.max_frame_size.load(Ordering::Relaxed),
                status,
                desc.errors,
                pkt_len,
                last_byte,
            ) {
                // NKSTAT > 0: em_tbi_adjust_stats(sc, pkt_len, sc->hw.mac_addr); kstat(4)
                // is not configured.
                len = len.saturating_sub(1);
            } else {
                accept_frame = false;
            }
        }

        if accept_frame {
            // Assign correct length to the current fragment
            m.m_len().set(u32::from(len));

            match (que.rx.fmp.get(), que.rx.lmp.get()) {
                (Some(fmp), Some(lmp)) => {
                    // Chain mbuf's together
                    m.m_flags().set(m.m_flags().get() & !M_PKTHDR);
                    // Adjust length of previous mbuf in chain if we received less than 4
                    // bytes in the last descriptor.
                    if prev_len_adj > 0 {
                        lmp.m_len()
                            .set(lmp.m_len().get().wrapping_sub(u32::from(prev_len_adj)));
                        fmp.m_pkthdr()
                            .len
                            .set(fmp.m_pkthdr().len.get() - i32::from(prev_len_adj));
                    }
                    lmp.m_next().set(Some(m));
                    que.rx.lmp.set(Some(m));
                    fmp.m_pkthdr()
                        .len
                        .set(fmp.m_pkthdr().len.get() + m.m_len().get() as i32);
                }
                _ => {
                    m.m_pkthdr().len.set(m.m_len().get() as i32);
                    que.rx.fmp.set(Some(m)); // Store the first mbuf
                    que.rx.lmp.set(Some(m));
                }
            }

            if eop && let Some(m) = que.rx.fmp.take() {
                em_receive_checksum(sc, &desc, m);
                if NVLAN > 0 && u32::from(desc.status) & E1000_RXD_STAT_VP != 0 {
                    m.m_pkthdr().ether_vtag.set(u16::from_le(desc.special));
                    m.m_flags().set(m.m_flags().get() | M_VLANTAG);
                }
                ml_enqueue(&ml, m);

                que.rx.lmp.set(None);
            }
        } else {
            que.rx.dropped_pkts.set(que.rx.dropped_pkts.get() + 1);

            if let Some(fmp) = que.rx.fmp.take() {
                m_freem(fmp);
                que.rx.lmp.set(None);
            }

            m_freem(m);
        }

        // Advance our pointers to the next descriptor.
        i += 1;
        if i == slots {
            i = 0;
        }
        if if_rxr_inuse(&que.rx.sc_rx_ring.get()) == 0 {
            break;
        }
    }

    bus_dmamap_sync(dmat, ring, 0, ring.dm_mapsize.get(), BUS_DMASYNC_PREREAD);

    que.rx.sc_rx_desc_tail.set(i);

    if ifiq_input(&ifp.if_rcv, &ml) {
        que.with_rx_ring(if_rxr_livelocked);
    }

    rv
}

/// `em_receive_checksum`: tells the stack which checksums the hardware verified.
pub fn em_receive_checksum(sc: &EmSoftc, rx_desc: &EmRxDesc, mp: &Mbuf) {
    let status = u32::from(rx_desc.status);
    let errors = u32::from(rx_desc.errors);
    let csum_flags = &mp.m_pkthdr().csum_flags;

    // 82543 or newer only
    if sc.mac_type() < em_82543 ||
        // Ignore Checksum bit is set
        status & E1000_RXD_STAT_IXSM != 0
    {
        csum_flags.set(0);
        return;
    }

    if status & E1000_RXD_STAT_IPCS != 0 {
        // Did it pass?
        if errors & E1000_RXD_ERR_IPE == 0 {
            // IP Checksum Good
            csum_flags.set(M_IPV4_CSUM_IN_OK);
        } else {
            csum_flags.set(0);
        }
    }

    if status & E1000_RXD_STAT_TCPCS != 0 {
        // Did it pass?
        if errors & E1000_RXD_ERR_TCPE == 0 {
            csum_flags.set(csum_flags.get() | M_TCP_CSUM_IN_OK | M_UDP_CSUM_IN_OK);
        }
    }
}

/// `em_enable_hw_vlans`: turns on the hardware VLAN tag insertion and stripping.
pub fn em_enable_hw_vlans(sc: &EmSoftc) {
    let regs = sc.regs();
    let ctrl = e1000_read_reg(regs, E1000_CTRL) | E1000_CTRL_VME;
    e1000_write_reg(regs, E1000_CTRL, ctrl);
}

/// `em_enable_intr`.
pub fn em_enable_intr(sc: &EmSoftc) {
    let regs = sc.regs();

    if sc.msix.get() != 0 {
        let mask = sc.msix_queuesmask.get() | sc.msix_linkmask.get();
        e1000_write_reg(regs, E1000_EIAC, mask);
        e1000_write_reg(regs, E1000_EIAM, mask);
        e1000_write_reg(regs, E1000_EIMS, mask);
        e1000_write_reg(regs, E1000_IMS, E1000_IMS_LSC);
    } else {
        e1000_write_reg(regs, E1000_IMS, IMS_ENABLE_MASK);
    }
}

/// `em_disable_intr`.
pub fn em_disable_intr(sc: &EmSoftc) {
    let regs = sc.regs();

    // The first version of 82542 had an errata where when link was forced it would stay up
    // even if the cable was disconnected. Sequence errors were used to detect the
    // disconnect and then the driver would unforce the link. This code is in the ISR. For
    // this to work correctly the Sequence error interrupt had to be enabled all the time.
    if sc.msix.get() != 0 {
        e1000_write_reg(regs, E1000_EIMC, !0);
        e1000_write_reg(regs, E1000_EIAC, 0);
    } else if sc.mac_type() == em_82542_rev2_0 {
        e1000_write_reg(regs, E1000_IMC, !E1000_IMC_RXSEQ);
    } else {
        e1000_write_reg(regs, E1000_IMC, 0xffff_ffff);
    }
}

/// `em_write_pci_cfg`: writes the 16-bit configuration word at `reg`.
pub fn em_write_pci_cfg(hw: &EmHw, reg: u32, value: &u16) {
    let pa = &hw.osdep().em_pa;

    let mut val = pci_conf_read(pa.pa_pc, pa.pa_tag, (reg & !0x3) as i32);
    if reg & 0x2 != 0 {
        val &= 0x0000_ffff;
        val |= u32::from(*value) << 16;
    } else {
        val &= 0xffff_0000;
        val |= u32::from(*value);
    }
    pci_conf_write(pa.pa_pc, pa.pa_tag, (reg & !0x3) as i32, val);
}

/// `em_read_pci_cfg`: reads the 16-bit configuration word at `reg`.
pub fn em_read_pci_cfg(hw: &EmHw, reg: u32, value: &mut u16) {
    let pa = &hw.osdep().em_pa;

    let val = pci_conf_read(pa.pa_pc, pa.pa_tag, (reg & !0x3) as i32);
    if reg & 0x2 != 0 {
        *value = ((val >> 16) & 0xffff) as u16;
    } else {
        *value = (val & 0xffff) as u16;
    }
}

/// `em_pci_set_mwi`: sets Memory Write and Invalidate in the command register.
pub fn em_pci_set_mwi(hw: &EmHw) {
    let pa = &hw.osdep().em_pa;

    pci_conf_write(
        pa.pa_pc,
        pa.pa_tag,
        PCI_COMMAND_STATUS_REG,
        u32::from(hw.pci_cmd_word) | CMD_MEM_WRT_INVALIDATE,
    );
}

/// `em_pci_clear_mwi`: clears it.
pub fn em_pci_clear_mwi(hw: &EmHw) {
    let pa = &hw.osdep().em_pa;

    pci_conf_write(
        pa.pa_pc,
        pa.pa_tag,
        PCI_COMMAND_STATUS_REG,
        u32::from(hw.pci_cmd_word) & !CMD_MEM_WRT_INVALIDATE,
    );
}

/// `em_read_pcie_cap_reg`: "We may eventually really do this, but its unnecessary for now so
/// we just return unsupported."
pub fn em_read_pcie_cap_reg(_hw: &EmHw, _reg: u32, _value: &mut u16) -> Result<(), i32> {
    Err(-E1000_NOT_IMPLEMENTED)
}

/// `em_fill_descriptors`: the 82544 coexistence workaround. A buffer whose end
/// (`SIZE[3:0] + ADDR[2:0]`) falls on 1 to 4 (transmit hang) or 9 to 0xc (DAC issue) is
/// split, so that the last four bytes go in a descriptor of their own.
pub fn em_fill_descriptors(address: u64, length: u32, desc_array: &mut DescArray) -> u32 {
    // Since issue is sensitive to length and address.
    // Let us first check the address...
    if length <= 4 {
        desc_array.descriptor[0].address = address;
        desc_array.descriptor[0].length = length;
        desc_array.elements = 1;
        return desc_array.elements;
    }
    let safe_terminator = ((address as u32 & 0x7) + (length & 0xF)) & 0xF;
    // if it does not fall between 0x1 to 0x4 and 0x9 to 0xC then return
    if safe_terminator == 0
        || (safe_terminator > 4 && safe_terminator < 9)
        || (safe_terminator > 0xC && safe_terminator <= 0xF)
    {
        desc_array.descriptor[0].address = address;
        desc_array.descriptor[0].length = length;
        desc_array.elements = 1;
        return desc_array.elements;
    }

    desc_array.descriptor[0].address = address;
    desc_array.descriptor[0].length = length - 4;
    desc_array.descriptor[1].address = address + u64::from(length - 4);
    desc_array.descriptor[1].length = 4;
    desc_array.elements = 2;
    desc_array.elements
}

/// `em_disable_aspm`: disables the L0s and L1 link states.
pub fn em_disable_aspm(sc: &EmSoftc) {
    let mac_type = sc.mac_type();

    match mac_type {
        em_82571 | em_82572 | em_82573 | em_82574 => {}
        _ => return,
    }

    let pa = sc.em_pa();
    let Some((offset, _)) = pci_get_capability(pa.pa_pc, pa.pa_tag, PCI_CAP_PCIEXPRESS) else {
        return;
    };

    // Disable PCIe Active State Power Management (ASPM).
    let mut val = pci_conf_read(pa.pa_pc, pa.pa_tag, offset + PCI_PCIE_LCSR);

    match mac_type {
        em_82571 | em_82572 => val &= !PCI_PCIE_LCSR_ASPM_L1,
        em_82573 | em_82574 => val &= !(PCI_PCIE_LCSR_ASPM_L0S | PCI_PCIE_LCSR_ASPM_L1),
        _ => {}
    }

    pci_conf_write(pa.pa_pc, pa.pa_tag, offset + PCI_PCIE_LCSR, val);
}

/// `em_flush_tx_ring`: removes all descriptors from the transmit ring. The ring itself is
/// given as the data of the next descriptor; the data does not matter, the hardware is about
/// to be reset.
pub fn em_flush_tx_ring(que: &EmQueue) {
    let sc = que.sc();
    let regs = sc.regs();
    let txd_lower = E1000_TXD_CMD_IFCS;
    let size: u16 = 512;
    let me = que.me.get();

    kassert!(!que.tx.sc_tx_desc_ring.get().is_null());

    let tctl = em_read_reg(regs, E1000_TCTL);
    em_write_reg(regs, E1000_TCTL, tctl | E1000_TCTL_EN);

    let mut head = que.tx.sc_tx_desc_head.load(Ordering::Relaxed);
    kassert!(em_read_reg(regs, e1000_tdt(me)) == head);

    que.txd_set(
        head,
        EmTxDesc {
            buffer_addr: que.tx.sc_tx_dma.map().dm_segs()[0].get().ds_addr as u64,
            lower: EmTxDescLower {
                data: (txd_lower | u32::from(size)).to_le(),
            },
            upper: EmTxDescUpper { data: 0 },
        },
    );

    // flush descriptors to memory before notifying the HW
    let o = sc.osdep();
    bus_space_barrier(
        o.mem_bus_space_tag,
        o.mem_bus_space_handle,
        0,
        0,
        BUS_SPACE_BARRIER_WRITE,
    );

    head += 1;
    if head == sc.sc_tx_slots.get() {
        head = 0;
    }
    que.tx.sc_tx_desc_head.store(head, Ordering::Release);

    em_write_reg(regs, e1000_tdt(me), head);
    bus_space_barrier(
        o.mem_bus_space_tag,
        o.mem_bus_space_handle,
        0,
        0,
        BUS_SPACE_BARRIER_READ | BUS_SPACE_BARRIER_WRITE,
    );
    usec_delay(250);
}

/// `em_flush_rx_ring`: marks all descriptors of the receive ring as consumed and disables
/// it.
pub fn em_flush_rx_ring(que: &EmQueue) {
    let sc = que.sc();
    let regs = sc.regs();
    let me = que.me.get();

    let rctl = em_read_reg(regs, E1000_RCTL);
    em_write_reg(regs, E1000_RCTL, rctl & !E1000_RCTL_EN);
    e1000_write_flush(regs);
    usec_delay(150);

    let mut rxdctl = em_read_reg(regs, e1000_rxdctl(me));
    // zero the lower 14 bits (prefetch and host thresholds)
    rxdctl &= 0xffff_c000;
    // update thresholds: prefetch threshold to 31, host threshold to 1 and make sure the
    // granularity is "descriptors" and not "cache lines"
    rxdctl |= 0x1F | (1 << 8) | E1000_RXDCTL_THRESH_UNIT_DESC;
    em_write_reg(regs, e1000_rxdctl(me), rxdctl);

    // momentarily enable the RX ring for the changes to take effect
    em_write_reg(regs, E1000_RCTL, rctl | E1000_RCTL_EN);
    e1000_write_flush(regs);
    usec_delay(150);
    em_write_reg(regs, E1000_RCTL, rctl & !E1000_RCTL_EN);
}

/// `em_flush_desc_rings`: empties the descriptor rings, which the i219 needs before a reset
/// (or it hangs until a PCI reset).
pub fn em_flush_desc_rings(sc: &EmSoftc) {
    let que = sc.que(0); // Use only first queue.
    let regs = sc.regs();
    let pa = sc.em_pa();

    // First, disable MULR fix in FEXTNVM11
    let fextnvm11 = em_read_reg(regs, E1000_FEXTNVM11) | E1000_FEXTNVM11_DISABLE_MULR_FIX;
    em_write_reg(regs, E1000_FEXTNVM11, fextnvm11);

    // do nothing if we're not in faulty state, or if the queue is empty
    let tdlen = em_read_reg(regs, e1000_tdlen(que.me.get()));
    let hang_state = pci_conf_read(pa.pa_pc, pa.pa_tag, PCICFG_DESC_RING_STATUS) as u16;
    if hang_state & FLUSH_DESC_REQUIRED == 0 || tdlen == 0 {
        return;
    }
    em_flush_tx_ring(que);

    // recheck, maybe the fault is caused by the rx ring
    let hang_state = pci_conf_read(pa.pa_pc, pa.pa_tag, PCICFG_DESC_RING_STATUS) as u16;
    if hang_state & FLUSH_DESC_REQUIRED != 0 {
        em_flush_rx_ring(que);
    }
}

/// `em_allocate_legacy`: maps and establishes the MSI (or INTx) interrupt.
pub fn em_allocate_legacy(sc: &'static EmSoftc) -> Result<(), Errno> {
    let pa = sc.em_pa();
    let pc = pa.pa_pc;

    let ih = match pci_intr_map_msi(pa) {
        Some(ih) => ih,
        None => {
            let Some(ih) = pci_intr_map(pa) else {
                printf(format_args!(": couldn't map interrupt\n"));
                return Err(Errno::ENXIO);
            };
            sc.legacy_irq.set(1);
            ih
        }
    };

    let intrstr = pci_intr_string(pc, ih);
    let Some(cookie) = pci_intr_establish(
        pc,
        ih,
        IPL_NET | IPL_MPSAFE,
        em_intr,
        ptr::from_ref(sc).cast_mut().cast(),
        sc.devname(),
    ) else {
        printf(format_args!(
            ": couldn't establish interrupt at {intrstr}\n"
        ));
        return Err(Errno::ENXIO);
    };
    sc.sc_intrhand.set(cookie.as_ptr());
    printf(format_args!(": {intrstr}"));

    Ok(())
}

// NKSTAT > 0: enum em_stat, struct em_counter, em_counters[], em_kstat_read,
// em_kstat_attach and em_tbi_adjust_stats; kstat(4) is not configured.

// MSIX/Multiqueue functions (SMALL_KERNEL is not defined)

/// `em_allocate_msix`: one MSI-X vector for the queue and one for the link, when
/// `em_enable_msix` is set and the MAC has them.
pub fn em_allocate_msix(sc: &'static EmSoftc) -> Result<(), Errno> {
    let pa = sc.em_pa();
    let pc = pa.pa_pc;
    let que = sc.que(0); // Use only first queue.

    if EM_ENABLE_MSIX.load(Ordering::Relaxed) == 0 {
        return Err(Errno::ENODEV);
    }

    match sc.mac_type() {
        em_82576 | em_82580 | em_i350 | em_i210 => {}
        _ => return Err(Errno::ENODEV),
    }

    let mut vec: u32 = 0;
    let Some(ih) = pci_intr_map_msix(pa, vec as i32) else {
        return Err(Errno::ENODEV);
    };
    sc.msix.set(1);

    que.me.set(vec);
    que.eims.set(1 << vec);
    let mut name = [0u8; 8];
    let _ = crate::kern::subr_prf::snprintf(&mut name, format_args!("{}:{}", sc.devname(), vec));
    que.name.set(name);

    let intrstr = pci_intr_string(pc, ih);
    let Some(tag) = pci_intr_establish(
        pc,
        ih,
        IPL_NET | IPL_MPSAFE,
        em_queue_intr_msix,
        ptr::from_ref(que).cast_mut().cast(),
        em_intr_name(&name),
    ) else {
        printf(format_args!(
            ": couldn't establish interrupt at {intrstr}\n"
        ));
        return Err(Errno::ENXIO);
    };
    que.tag.set(tag.as_ptr());

    // Setup linkvector, use last queue vector + 1
    vec += 1;
    sc.msix_linkvec.set(vec);
    let Some(ih) = pci_intr_map_msix(pa, vec as i32) else {
        printf(format_args!(": couldn't map link vector\n"));
        return Err(Errno::ENXIO);
    };

    let intrstr = pci_intr_string(pc, ih);
    let Some(cookie) = pci_intr_establish(
        pc,
        ih,
        IPL_NET | IPL_MPSAFE,
        em_link_intr_msix,
        ptr::from_ref(sc).cast_mut().cast(),
        sc.devname(),
    ) else {
        printf(format_args!(
            ": couldn't establish interrupt at {intrstr}\n"
        ));
        return Err(Errno::ENXIO);
    };
    sc.sc_intrhand.set(cookie.as_ptr());
    printf(format_args!(
        ", {intrstr}, {vec} queue{}",
        if vec > 1 { "s" } else { "" }
    ));

    Ok(())
}

/// `em_queue_intr_msix`: the interrupt of a queue (not the link). The EICR bit that maps
/// to the EIMS bit expresses both RX and TX, so both are done; the EICR bits are
/// autocleared and cannot be read.
pub fn em_queue_intr_msix(vque: *mut c_void) -> i32 {
    // SAFETY: em_allocate_msix established the handler with the queue as its argument.
    let que = unsafe { &*vque.cast::<EmQueue>().cast_const() };
    let sc = que.sc();
    let ifp = sc.ifp();

    if ifp.if_flags.get() & IFF_RUNNING != 0 {
        em_txeof(que);
        if em_rxeof(que) {
            em_rxrefill_locked(que);
        }
    }

    em_enable_queue_intr_msix(que);

    1
}

/// `em_link_intr_msix`: the link vector's interrupt.
pub fn em_link_intr_msix(arg: *mut c_void) -> i32 {
    // SAFETY: em_allocate_msix established the handler with the softc as its argument.
    let sc = unsafe { &*arg.cast::<EmSoftc>().cast_const() };
    let regs = sc.regs();

    let icr = e1000_read_reg(regs, E1000_ICR);

    // Link status change
    if icr & E1000_ICR_LSC != 0 {
        kernel_lock();
        {
            let mut hw = sc.em_hw();
            hw.get_link_status = true;
            let _ = em_check_for_link(&mut hw);
        }
        em_update_link_status(sc);
        kernel_unlock();
    }

    // Re-arm unconditionally
    e1000_write_reg(regs, E1000_IMS, E1000_ICR_LSC);
    e1000_write_reg(regs, E1000_EIMS, sc.msix_linkmask.get());

    1
}

/// `em_setup_queues_msix`: maps the queues into MSI-X interrupt vectors.
pub fn em_setup_queues_msix(sc: &EmSoftc) -> Result<(), Errno> {
    let regs = sc.regs();
    let mac_type = sc.mac_type();

    kassert!(sc.msix.get() != 0);

    // First turn on RSS capability
    if mac_type != em_82575 {
        e1000_write_reg(
            regs,
            E1000_GPIE,
            E1000_GPIE_MSIX_MODE | E1000_GPIE_EIAME | E1000_GPIE_PBA | E1000_GPIE_NSICR,
        );
    }

    // Turn on MSIX
    match mac_type {
        em_82580 | em_i350 | em_i210 => {
            // RX entries
            //
            // Note, this maps Queues into MSIX vectors, it works fine. The funky calculation
            // of offsets and checking if que->me is odd is due to the weird register
            // distribution, the datasheet explains it well.
            for que in sc.queues() {
                let me = que.me.get();
                let index = me >> 1;
                let mut ivar = e1000_read_reg_array(regs, E1000_IVAR0, index);
                if me & 1 != 0 {
                    ivar &= 0xFF00_FFFF;
                    ivar |= (me | E1000_IVAR_VALID) << 16;
                } else {
                    ivar &= 0xFFFF_FF00;
                    ivar |= me | E1000_IVAR_VALID;
                }
                e1000_write_reg_array(regs, E1000_IVAR0, index, ivar);
            }

            // TX entries
            for que in sc.queues() {
                let me = que.me.get();
                let index = me >> 1;
                let mut ivar = e1000_read_reg_array(regs, E1000_IVAR0, index);
                if me & 1 != 0 {
                    ivar &= 0x00FF_FFFF;
                    ivar |= (me | E1000_IVAR_VALID) << 24;
                } else {
                    ivar &= 0xFFFF_00FF;
                    ivar |= (me | E1000_IVAR_VALID) << 8;
                }
                e1000_write_reg_array(regs, E1000_IVAR0, index, ivar);
                sc.msix_queuesmask
                    .set(sc.msix_queuesmask.get() | que.eims.get());
            }

            // And for the link interrupt
            let ivar = (sc.msix_linkvec.get() | E1000_IVAR_VALID) << 8;
            sc.msix_linkmask.set(1 << sc.msix_linkvec.get());
            e1000_write_reg(regs, E1000_IVAR_MISC, ivar);
        }
        em_82576 => {
            // RX entries
            for que in sc.queues() {
                let me = que.me.get();
                let index = me & 0x7; // Each IVAR has two entries
                let mut ivar = e1000_read_reg_array(regs, E1000_IVAR0, index);
                if me < 8 {
                    ivar &= 0xFFFF_FF00;
                    ivar |= me | E1000_IVAR_VALID;
                } else {
                    ivar &= 0xFF00_FFFF;
                    ivar |= (me | E1000_IVAR_VALID) << 16;
                }
                e1000_write_reg_array(regs, E1000_IVAR0, index, ivar);
                sc.msix_queuesmask
                    .set(sc.msix_queuesmask.get() | que.eims.get());
            }
            // TX entries
            for que in sc.queues() {
                let me = que.me.get();
                let index = me & 0x7; // Each IVAR has two entries
                let mut ivar = e1000_read_reg_array(regs, E1000_IVAR0, index);
                if me < 8 {
                    ivar &= 0xFFFF_00FF;
                    ivar |= (me | E1000_IVAR_VALID) << 8;
                } else {
                    ivar &= 0x00FF_FFFF;
                    ivar |= (me | E1000_IVAR_VALID) << 24;
                }
                e1000_write_reg_array(regs, E1000_IVAR0, index, ivar);
                sc.msix_queuesmask
                    .set(sc.msix_queuesmask.get() | que.eims.get());
            }

            // And for the link interrupt
            let ivar = (sc.msix_linkvec.get() | E1000_IVAR_VALID) << 8;
            sc.msix_linkmask.set(1 << sc.msix_linkvec.get());
            e1000_write_reg(regs, E1000_IVAR_MISC, ivar);
        }
        _ => panic(format_args!("unsupported mac")),
    }

    // Set the starting interrupt rate
    let mut newitr = (4_000_000 / MAX_INTS_PER_SEC) & 0x7FFC;

    if mac_type == em_82575 {
        newitr |= newitr << 16;
    } else {
        newitr |= E1000_EITR_CNT_IGNR;
    }

    for que in sc.queues() {
        e1000_write_reg(regs, e1000_eitr(que.me.get()), newitr);
    }

    Ok(())
}

/// `em_enable_queue_intr_msix`.
pub fn em_enable_queue_intr_msix(que: &EmQueue) {
    e1000_write_reg(que.sc().regs(), E1000_EIMS, que.eims.get());
}

/// `em_allocate_desc_rings`: the transmit and receive descriptor rings of every queue.
pub fn em_allocate_desc_rings(sc: &EmSoftc) -> Result<(), Errno> {
    for que in sc.queues() {
        // Allocate Transmit Descriptor ring
        if em_dma_malloc(
            sc,
            sc.sc_tx_slots.get() as usize * size_of::<EmTxDesc>(),
            &que.tx.sc_tx_dma,
        )
        .is_err()
        {
            printf(format_args!(
                "{}: Unable to allocate tx_desc memory\n",
                sc.devname()
            ));
            return Err(Errno::ENOMEM);
        }
        que.tx
            .sc_tx_desc_ring
            .set(que.tx.sc_tx_dma.dma_vaddr.get().cast());

        // Allocate Receive Descriptor ring
        if em_dma_malloc(
            sc,
            sc.sc_rx_slots.get() as usize * size_of::<EmRxDesc>(),
            &que.rx.sc_rx_dma,
        )
        .is_err()
        {
            printf(format_args!(
                "{}: Unable to allocate rx_desc memory\n",
                sc.devname()
            ));
            return Err(Errno::ENOMEM);
        }
        que.rx
            .sc_rx_desc_ring
            .set(que.rx.sc_rx_dma.dma_vaddr.get().cast());
    }

    Ok(())
}

/// `em_get_sffpage`: reads a page of the SFP module's EEPROM or diagnostics (82575 to
/// i350).
pub fn em_get_sffpage(sc: &EmSoftc, sff: &mut IfSffpage) -> Result<(), Errno> {
    let hw = sc.em_hw();

    if !matches!(
        hw.mac_type,
        em_82575 | em_82580 | em_82576 | em_i210 | em_i350
    ) {
        return Err(Errno::ENODEV);
    }

    let off = if sff.sff_addr == IFSFF_ADDR_EEPROM {
        e1000_i2ccmd_sfp_data_addr(0)
    } else if sff.sff_addr == IFSFF_ADDR_DDM {
        e1000_i2ccmd_sfp_diag_addr(0)
    } else {
        return Err(Errno::EIO);
    };

    for (i, byte) in sff.sff_data.iter_mut().enumerate() {
        if em_read_sfp_data_byte(&hw, (usize::from(off) + i) as u16, byte).is_err() {
            return Err(Errno::EIO);
        }
    }

    Ok(())
}

const _: () = {
    // em_tso_setup, em_tx_ctx_setup and em_transmit_checksum_setup cast a transmit slot to
    // a context descriptor, as the C does.
    assert!(size_of::<E1000AdvTxContextDesc>() == size_of::<EmTxDesc>());
    assert!(size_of::<EmContextDesc>() == size_of::<EmTxDesc>());
    assert!(align_of::<E1000AdvTxContextDesc>() <= align_of::<EmTxDesc>());
    assert!(align_of::<EmContextDesc>() <= align_of::<EmTxDesc>());
    assert!(size_of::<EmTxDesc>() == 16);
    assert!(size_of::<EmRxDesc>() == 16);
    // sizeof(struct ip), the IPv4 header the context descriptor describes.
    assert!(size_of::<Ip>() == 20);
};

#[cfg(test)]
mod tests;
