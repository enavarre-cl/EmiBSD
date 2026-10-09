/* <LICENSES> */
/*
 * Copyright (c) 2026 Emilio Navarrete Lineros <enavarre@outlook.com>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*
 * Copyright (c) 2013 Tsubai Masanari
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */
/* </LICENSES> */

/* <CODE> */
//! vmx(4): the driver of VMware's VMXNET3 virtual NIC (`vmx* at pci?`), which QEMU emulates
//! as `vmxnet3`.
//!
//! Upstream: sys/dev/pci/if_vmx.c @ 3ce1f3f79392
//!
//! The device is driven through two BARs (BAR0 the per-queue doorbells and interrupt
//! masks, BAR1 the commands and the address of the driver-shared area) and DMA memory: the
//! driver-shared structure, one transmit and one receive queue-shared structure per queue,
//! the multicast table and, with several queues, the RSS configuration. Each queue has a
//! transmit command ring with its completion ring, and two receive command rings (heads and
//! bodies) with one completion ring; ownership of a descriptor passes by a generation bit.
//! With MSI-X and spare vectors the queues get their own vectors through `intrmap(9)`
//! (vector 0 is the event interrupt); otherwise one MSI-X, MSI or INTx vector serves
//! queue 0 and the events.
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the device first and the `struct arpcom` inside, made of
//!   `Cell`s and atomics (all-zero valid), as the NIC rows of `docs/C_TO_RUST.md` say; it is
//!   reached as `&'static` (softcs are never freed while the interface exists). The queues
//!   (`sc_q`) are one zeroed `mallocarray`, as in C, reached through a bounds-checked
//!   accessor; their mbuf and dmamap arrays are arrays of `Cell<Option<..>>`.
//! - Locking as in C, made explicit: `vmxnet3_start` runs on whatever CPU serves the send
//!   queue, the queue interrupts (`IPL_NET | IPL_MPSAFE`) on the CPUs `intrmap(9)` picks,
//!   the event interrupt takes the kernel lock around `vmxnet3_evintr` as the C does, and
//!   the ioctls, `init` and `stop` run under the kernel lock. The transmit ring's `prod` and
//!   `cons` (`volatile u_int` in C) are atomics; each transmit slot belongs to the send
//!   queue or to `vmxnet3_txintr` by those indices, as in C. The receive rings' fill state
//!   is under their `mtx`, as in C.
//! - The descriptor rings and the structures shared with the device are DMA memory reached
//!   through raw pointers with bounds-checked accessors; every descriptor is read and
//!   written whole and volatile (a `|=` on a descriptor is a read, a change and a write),
//!   as em(4) and re(4) do; a completion descriptor's generation word is read before the
//!   rest of it. The shared structures' members are read and written one by one, volatile;
//!   where the C `bzero`s a structure and fills it, a whole value is written.
//!   `vmxnet3_tx_offload` changes a copy of the start-of-packet descriptor, written back
//!   before the generation bit is flipped. The memsets of the rings are volatile writes of
//!   zeroed descriptors.
//! - `struct vmxnet3_comp_ring`'s union of `txcd` and `rxcd` is two pointers; a ring sets
//!   the one its queue uses.
//! - `vmxnet3_rxintr` applies `if_rxr_livelocked` under the ring's `mtx` (the C changes the
//!   ring's accounting outside it, racing `vmxnet3_rxfill_tick`).
//! - `vmxnet3_rxstop` unloads a slot's map before freeing its mbuf (the C frees first): a
//!   loaded map's buffer must stay allocated until the unload (`bus_dmamap_load_mbuf`'s
//!   contract).
//! - `vmxnet3_attach` stops with "failed to map interrupt" when the device has MSI-X
//!   vectors but vector 0 cannot be mapped (the C goes on with an uninitialised handle).
//!   `vmxnet3_dma_init` fails when the RSS configuration cannot be allocated (the C writes
//!   through NULL).
//! - `kstat(4)` is not configured (`NKSTAT` 0): `vmx_kstat_init`, `vmx_kstat_read`,
//!   `vmx_kstat_create`, `vmx_kstat_txstats`, `vmx_kstat_rxstats`, `vmx_kstat_rate`,
//!   `struct vmx_kstat_tpl` and its tables, the softc's `sc_kstat_lock` and
//!   `sc_kstat_updated` and the queues' `txkstat`/`rxkstat` are compiled out, as in such a C
//!   kernel; comments mark the call sites. `vmx_dmamem_free` is `#ifdef notyet` in C and is
//!   left out likewise.
//! - `vlan(4)` is not configured: the `#if NVLAN > 0` blocks are behind the constant
//!   [`NVLAN`] (0), so `IFCAP_VLAN_HWTAGGING` and `UPT1_F_VLAN` are not offered and tags are
//!   neither inserted nor taken from the descriptors. `NBPFILTER` is configured: the
//!   per-queue taps are attached with `bpfxattach` when the queues have their own vectors.
//! - `offsetof(struct tcphdr, th_sum)` and `offsetof(struct udphdr, uh_sum)` are the
//!   constants 16 and 6, `sizeof(struct ip)` and `sizeof(struct tcphdr)` 20:
//!   `netinet/tcp.h` and `netinet/udp.h` are not ported.
//! - Functions returning 0 or an errno return `Result<(), Errno>`; `vmxnet3_dma_init`,
//!   `vmxnet3_alloc_txring`, `vmxnet3_alloc_rxring` and `vmx_dmamem_alloc` return
//!   `Err(ENOMEM)` for the C's -1 and 1; `vmxnet3_dma_allocmem` returns the mapping and its
//!   bus address, or `Err`.
//! - A queue's MSI-X interrupt is established under a copy of its `intrname` that is never
//!   freed (`vmx_intr_name`), as em(4) does: `evcount(9)` keeps the pointer.
//! - `intr_barrier` is skipped for an interrupt that was not established.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicU32, Ordering};

use crate::dev::pci::if_vmxreg::*;
use crate::dev::pci::pci::{pci_intr_msix_count, pci_matchbyid};
use crate::dev::pci::pci_map::{pci_mapreg_map, pci_mapreg_type};
use crate::dev::pci::pcidevs::{PCI_PRODUCT_VMWARE_NET_3, PCI_VENDOR_VMWARE};
use crate::dev::pci::pcireg::{PciProductId, PciVendorId};
use crate::dev::pci::pcivar::{PciAttachArgs, PciMatchid};
use crate::kassert;
use crate::kern::kern_intrmap::{Intrmap, intrmap_count, intrmap_cpu, intrmap_create};
use crate::kern::kern_lock::{mtx_enter, mtx_enter_try, mtx_init, mtx_leave};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_timeout::{timeout_add, timeout_del, timeout_set};
use crate::kern::subr_prf::{Str, panic, printf, snprintf};
use crate::kern::uipc_mbuf::{m_adj, m_clget, m_defrag, m_freem, m_pullup, ml_enqueue};
use crate::machine::bus::{
    BUS_DMA_64BIT, BUS_DMA_ALLOCNOW, BUS_DMA_NOWAIT, BUS_DMA_STREAMING, BUS_DMA_WAITOK,
    BUS_DMA_ZERO, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE, BUS_DMASYNC_PREREAD,
    BUS_DMASYNC_PREWRITE, BusAddr, BusDmaSegment, BusDmaTag, BusDmamap, BusSize, BusSpaceHandle,
    BusSpaceTag, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load, bus_dmamap_load_mbuf,
    bus_dmamap_sync, bus_dmamap_unload, bus_dmamem_alloc, bus_dmamem_free, bus_dmamem_map,
    bus_dmamem_unmap, bus_space_read_4, bus_space_write_4,
};
use crate::machine::intr::{IPL_MPSAFE, IPL_NET, intr_barrier, splnet, splx};
use crate::machine::pci_machdep::{
    PciIntrFn, PciIntrHandle, pci_intr_establish, pci_intr_establish_cpu, pci_intr_map,
    pci_intr_map_msi, pci_intr_map_msix, pci_intr_string,
};
use crate::net::bpf::{BPF_DIRECTION_OUT, DLT_EN10MB, bpf_mtap_ether, bpfxattach};
use crate::net::if_::{
    IF_MAX_VECTORS, IFCAP_CSUM_TCPv4, IFCAP_CSUM_TCPv6, IFCAP_CSUM_UDPv4, IFCAP_CSUM_UDPv6,
    IFCAP_LRO, IFCAP_TSOv4, IFCAP_TSOv6, IFCAP_VLAN_HWTAGGING, IFCAP_VLAN_MTU, IFF_ALLMULTI,
    IFF_BROADCAST, IFF_MULTICAST, IFF_PROMISC, IFF_RUNNING, IFF_SIMPLEX, IFF_UP, IFNAMSIZ,
    IFXF_LRO, IFXF_MBUF_64BIT, IFXF_MPSAFE, IfRxring, IfRxringInfo, Ifmediareq, Ifreq,
    LINK_STATE_DOWN, LINK_STATE_UP, if_attach, if_attach_iqueues, if_attach_queues, if_gbps,
    if_link_state_change, if_mbps, if_rxr_get, if_rxr_info_ioctl, if_rxr_init, if_rxr_livelocked,
};
use crate::net::if_ethersubr::{ether_extract_headers, ether_ifattach, ether_ioctl, ether_sprintf};
use crate::net::if_media::{
    IFM_10G_T, IFM_1000_T, IFM_ACTIVE, IFM_AUTO, IFM_AVALID, IFM_ETHER, IFM_FDX, IFM_IMASK,
    Ifmedia, ifmedia_add, ifmedia_init, ifmedia_ioctl, ifmedia_set,
};
use crate::net::if_var::{Ifnet, if_rxr_inuse, if_rxr_put};
use crate::net::ifq::{
    Ifiqueue, Ifqueue, ifiq_input, ifq_clr_oactive, ifq_dequeue, ifq_init_maxlen, ifq_is_oactive,
    ifq_purge, ifq_restart, ifq_set_oactive,
};
use crate::netinet::if_ether::{
    Arpcom, ETHER_ADDR_LEN, ETHER_ALIGN, ETHER_HDR_LEN, EtherExtracted, EtherMultistep,
    ether_first_multi, ether_next_multi,
};
use crate::netinet::ip::Ip;
use crate::netinet::tcp_var::{TcpstatCounters, tcpstat_add, tcpstat_inc};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_IFNET, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_CANFAIL, M_DEVBUF, M_NOWAIT, M_TEMP, M_WAITOK, M_ZERO};
use crate::sys::mbuf::{
    M_DONTWAIT, M_FLOWID, M_IPV4_CSUM_IN_OK, M_PKTHDR, M_TCP_CSUM_IN_OK, M_TCP_CSUM_OUT, M_TCP_TSO,
    M_UDP_CSUM_IN_OK, M_UDP_CSUM_OUT, M_VLANTAG, MAXMCLBYTES, MHLEN, Mbuf, MbufList, mtod,
};
use crate::sys::mutex::{Mutex, mutex_assert_locked};
use crate::sys::sockio::{
    SIOCGIFMEDIA, SIOCGIFRXR, SIOCSIFADDR, SIOCSIFFLAGS, SIOCSIFMEDIA, SIOCSIFXFLAGS,
};
use crate::sys::systm::{kernel_lock, kernel_unlock};
use crate::sys::timeout::Timeout;

/// `NVLAN` (`vlan.h`): `vlan(4)` is not configured.
pub const NVLAN: i32 = 0;

/// `VMX_MAX_QUEUES`.
pub const VMX_MAX_QUEUES: usize = if VMXNET3_MAX_TX_QUEUES < VMXNET3_MAX_RX_QUEUES {
    VMXNET3_MAX_TX_QUEUES
} else {
    VMXNET3_MAX_RX_QUEUES
};

/// `NTXDESC`: tx ring size.
pub const NTXDESC: u32 = 512;
/// `NTXSEGS`: tx descriptors per packet.
pub const NTXSEGS: u32 = 8;
/// `NRXDESC`.
pub const NRXDESC: u32 = 512;
/// `NTXCOMPDESC`.
pub const NTXCOMPDESC: u32 = NTXDESC;
/// `NRXCOMPDESC`: ring1 + ring2.
pub const NRXCOMPDESC: u32 = NRXDESC * 2;

/// `VMXNET3_DRIVER_VERSION`.
pub const VMXNET3_DRIVER_VERSION: u32 = 0x00010000;

/// `VMX_TX_GEN`.
pub const VMX_TX_GEN: u32 = (VMXNET3_TX_GEN_M << VMXNET3_TX_GEN_S).to_le();
/// `VMX_TXC_GEN`.
pub const VMX_TXC_GEN: u32 = (VMXNET3_TXC_GEN_M << VMXNET3_TXC_GEN_S).to_le();
/// `VMX_RX_GEN`.
pub const VMX_RX_GEN: u32 = (VMXNET3_RX_GEN_M << VMXNET3_RX_GEN_S).to_le();
/// `VMX_RXC_GEN`.
pub const VMX_RXC_GEN: u32 = (VMXNET3_RXC_GEN_M << VMXNET3_RXC_GEN_S).to_le();

/// `JUMBO_LEN`.
pub const JUMBO_LEN: u32 = (16 * 1024) - 1;

/// The size of the multicast table: 682 addresses.
const VMX_MCAST_LEN: usize = 682 * ETHER_ADDR_LEN;

/// `offsetof(struct tcphdr, th_sum)` (`netinet/tcp.h`, not ported).
const TCPHDR_TH_SUM: u32 = 16;
/// `offsetof(struct udphdr, uh_sum)` (`netinet/udp.h`, not ported).
const UDPHDR_UH_SUM: u32 = 6;
/// `sizeof(struct tcphdr)` without options (`netinet/tcp.h`, not ported).
const TCPHDR_LEN: usize = 20;

/// A slot of a ring's mbuf array.
type MbufSlot = Cell<Option<&'static Mbuf>>;
/// A slot of a ring's dmamap array.
type MapSlot = Cell<Option<&'static BusDmamap>>;

/// `struct vmx_dmamem`: a DMA area of one segment, mapped and loaded.
#[repr(C)]
pub struct VmxDmamem {
    /// `vdm_map`.
    pub vdm_map: Cell<Option<&'static BusDmamap>>,
    /// `vdm_seg`.
    pub vdm_seg: Cell<BusDmaSegment>,
    /// `vdm_nsegs`.
    pub vdm_nsegs: Cell<i32>,
    /// `vdm_size`.
    pub vdm_size: Cell<BusSize>,
    /// `vdm_kva`.
    pub vdm_kva: Cell<*mut u8>,
}

impl VmxDmamem {
    /// `VMX_DMA_MAP(vdm)`.
    pub fn map(&self) -> &'static BusDmamap {
        match self.vdm_map.get() {
            Some(m) => m,
            None => panic(format_args!("vmx: dma area without a map")),
        }
    }

    /// `VMX_DMA_DVA(vdm)`: the area's bus address.
    pub fn dva(&self) -> BusAddr {
        self.map().dm_segs()[0].get().ds_addr
    }

    /// `VMX_DMA_KVA(vdm)`.
    pub fn kva(&self) -> *mut u8 {
        self.vdm_kva.get()
    }

    /// `VMX_DMA_LEN(vdm)`.
    pub fn len(&self) -> BusSize {
        self.vdm_size.get()
    }

    /// Whether the area is empty (`VMX_DMA_LEN(vdm) == 0`).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// `struct vmxnet3_txring`: a queue's transmit command ring. `vmxnet3_start` fills it from
/// `prod`, `vmxnet3_txintr` reclaims it from `cons`.
#[repr(C)]
pub struct Vmxnet3Txring {
    /// `dmamem`.
    pub dmamem: VmxDmamem,
    /// `m`.
    pub m: [MbufSlot; NTXDESC as usize],
    /// `dmap`.
    pub dmap: [MapSlot; NTXDESC as usize],
    /// `txd`: the descriptors, in `dmamem`.
    pub txd: Cell<*mut Vmxnet3Txdesc>,
    /// `gen`: the generation the send queue writes.
    pub r#gen: Cell<u32>,
    /// `prod`.
    pub prod: AtomicU32,
    /// `cons`.
    pub cons: AtomicU32,
}

impl Vmxnet3Txring {
    /// `&ring->txd[i]`, a slot of the DMA ring.
    fn txd_ptr(&self, i: u32) -> *mut Vmxnet3Txdesc {
        let base = self.txd.get();
        if base.is_null() || i >= NTXDESC {
            panic(format_args!("vmx: bad tx descriptor {i}"));
        }
        // SAFETY: vmxnet3_alloc_txring mapped NTXDESC descriptors at `base`.
        unsafe { base.add(i as usize) }
    }

    /// `ring->txd[i]`.
    fn txd_get(&self, i: u32) -> Vmxnet3Txdesc {
        // SAFETY: a slot of the mapped ring (`txd_ptr`); the descriptor is plain data.
        unsafe { ptr::read_volatile(self.txd_ptr(i)) }
    }

    /// `ring->txd[i] = d`.
    fn txd_set(&self, i: u32, d: Vmxnet3Txdesc) {
        // SAFETY: a slot of the mapped ring (`txd_ptr`).
        unsafe { ptr::write_volatile(self.txd_ptr(i), d) }
    }

    /// `ring->dmap[i]`, which vmxnet3_alloc_txring created.
    fn map(&self, i: u32) -> &'static BusDmamap {
        match self.dmap[i as usize].get() {
            Some(m) => m,
            None => panic(format_args!("vmx: tx slot {i} without a dma map")),
        }
    }
}

/// `struct vmxnet3_rxring`: one of a queue's receive command rings. Its fill state
/// (`fill`, `gen`, `rxr`, the slots it fills) is under `mtx`; `vmxnet3_rxintr` takes the
/// slots the device completed.
#[repr(C)]
pub struct Vmxnet3Rxring {
    /// `sc`.
    pub sc: Cell<*const Vmxnet3Softc>,
    /// `rs`: copy of the rxqueue rs.
    pub rs: Cell<*mut Vmxnet3RxqShared>,
    /// `dmamem`.
    pub dmamem: VmxDmamem,
    /// `m`.
    pub m: [MbufSlot; NRXDESC as usize],
    /// `dmap`.
    pub dmap: [MapSlot; NRXDESC as usize],
    /// `mtx`.
    pub mtx: Mutex,
    /// `rxr`.
    pub rxr: Cell<IfRxring>,
    /// `refill`.
    pub refill: Timeout,
    /// `rxd`: the descriptors, in `dmamem`.
    pub rxd: Cell<*mut Vmxnet3Rxdesc>,
    /// `rxh`: the ring's head register in BAR0.
    pub rxh: Cell<BusSize>,
    /// `fill`.
    pub fill: Cell<u32>,
    /// `gen`.
    pub r#gen: Cell<u32>,
    /// `rid`.
    pub rid: Cell<u8>,
}

impl Vmxnet3Rxring {
    /// `ring->sc`.
    fn sc(&self) -> &'static Vmxnet3Softc {
        let p = self.sc.get();
        if p.is_null() {
            panic(format_args!("vmx: rx ring without its softc"));
        }
        // SAFETY: vmxnet3_alloc_rxring points it at the softc that holds the ring; softcs
        // are never freed while their interface exists.
        unsafe { &*p }
    }

    /// `&ring->rxd[i]`, a slot of the DMA ring.
    fn rxd_ptr(&self, i: u32) -> *mut Vmxnet3Rxdesc {
        let base = self.rxd.get();
        if base.is_null() || i >= NRXDESC {
            panic(format_args!("vmx: bad rx descriptor {i}"));
        }
        // SAFETY: vmxnet3_alloc_rxring mapped NRXDESC descriptors at `base`.
        unsafe { base.add(i as usize) }
    }

    /// `ring->rxd[i]`.
    fn rxd_get(&self, i: u32) -> Vmxnet3Rxdesc {
        // SAFETY: a slot of the mapped ring (`rxd_ptr`); the descriptor is plain data.
        unsafe { ptr::read_volatile(self.rxd_ptr(i)) }
    }

    /// `ring->rxd[i] = d`.
    fn rxd_set(&self, i: u32, d: Vmxnet3Rxdesc) {
        // SAFETY: a slot of the mapped ring (`rxd_ptr`).
        unsafe { ptr::write_volatile(self.rxd_ptr(i), d) }
    }

    /// `ring->dmap[i]`, which vmxnet3_alloc_rxring created.
    fn map(&self, i: u32) -> &'static BusDmamap {
        match self.dmap[i as usize].get() {
            Some(m) => m,
            None => panic(format_args!("vmx: rx slot {i} without a dma map")),
        }
    }

    /// `ring->rs->update_rxhead`.
    fn rs_update_rxhead(&self) -> u8 {
        let rs = self.rs.get();
        if rs.is_null() {
            panic(format_args!("vmx: rx ring without its shared structure"));
        }
        // SAFETY: vmxnet3_dma_init carved the queue's shared structure out of the DMA area,
        // which stays mapped; the device may change it, so it is read volatile.
        unsafe { ptr::read_volatile(ptr::addr_of!((*rs).update_rxhead)) }
    }

    /// `ring->rxr`, changed by `f`.
    fn with_rxr<R>(&self, f: impl FnOnce(&mut IfRxring) -> R) -> R {
        let mut r = self.rxr.get();
        let v = f(&mut r);
        self.rxr.set(r);
        v
    }
}

/// `struct vmxnet3_comp_ring`: a completion ring the device writes.
#[repr(C)]
pub struct Vmxnet3CompRing {
    /// `dmamem`.
    pub dmamem: VmxDmamem,
    /// `txcd`: the transmit completion descriptors, for a transmit queue.
    pub txcd: Cell<*mut Vmxnet3Txcompdesc>,
    /// `rxcd`: the receive completion descriptors, for a receive queue.
    pub rxcd: Cell<*mut Vmxnet3Rxcompdesc>,
    /// `next`.
    pub next: Cell<u32>,
    /// `gen`.
    pub r#gen: Cell<u32>,
    /// `sendmp`.
    pub sendmp: Cell<Option<&'static Mbuf>>,
    /// `lastmp`.
    pub lastmp: Cell<Option<&'static Mbuf>>,
}

impl Vmxnet3CompRing {
    /// `&comp_ring->txcd[i]`.
    fn txcd_ptr(&self, i: u32) -> *mut Vmxnet3Txcompdesc {
        let base = self.txcd.get();
        if base.is_null() || i >= NTXCOMPDESC {
            panic(format_args!("vmx: bad tx completion descriptor {i}"));
        }
        // SAFETY: vmxnet3_alloc_txring mapped NTXCOMPDESC descriptors at `base`.
        unsafe { base.add(i as usize) }
    }

    /// `comp_ring->txcd[i].txc_word3`, read alone (it holds the generation).
    fn txcd_word3(&self, i: u32) -> u32 {
        // SAFETY: a slot of the mapped ring (`txcd_ptr`).
        unsafe { ptr::read_volatile(ptr::addr_of!((*self.txcd_ptr(i)).txc_word3)) }
    }

    /// `comp_ring->txcd[i]`.
    fn txcd_get(&self, i: u32) -> Vmxnet3Txcompdesc {
        // SAFETY: a slot of the mapped ring (`txcd_ptr`); the descriptor is plain data.
        unsafe { ptr::read_volatile(self.txcd_ptr(i)) }
    }

    /// `&comp_ring->rxcd[i]`.
    fn rxcd_ptr(&self, i: u32) -> *mut Vmxnet3Rxcompdesc {
        let base = self.rxcd.get();
        if base.is_null() || i >= NRXCOMPDESC {
            panic(format_args!("vmx: bad rx completion descriptor {i}"));
        }
        // SAFETY: vmxnet3_alloc_rxring mapped NRXCOMPDESC descriptors at `base`.
        unsafe { base.add(i as usize) }
    }

    /// `comp_ring->rxcd[i].rxc_word3`, read alone (it holds the generation).
    fn rxcd_word3(&self, i: u32) -> u32 {
        // SAFETY: a slot of the mapped ring (`rxcd_ptr`).
        unsafe { ptr::read_volatile(ptr::addr_of!((*self.rxcd_ptr(i)).rxc_word3)) }
    }

    /// `comp_ring->rxcd[i]`.
    fn rxcd_get(&self, i: u32) -> Vmxnet3Rxcompdesc {
        // SAFETY: a slot of the mapped ring (`rxcd_ptr`); the descriptor is plain data.
        unsafe { ptr::read_volatile(self.rxcd_ptr(i)) }
    }
}

/// `struct vmxnet3_txqueue`.
#[repr(C, align(64))]
pub struct Vmxnet3Txqueue {
    /// `sc`: never set, as in C ("sigh").
    pub sc: Cell<*const Vmxnet3Softc>,
    /// `cmd_ring`.
    pub cmd_ring: Vmxnet3Txring,
    /// `comp_ring`.
    pub comp_ring: Vmxnet3CompRing,
    /// `ts`: the queue's shared structure, in the DMA area.
    pub ts: Cell<*mut Vmxnet3TxqShared>,
    /// `ifq`.
    pub ifq: Cell<Option<&'static Ifqueue>>,
    /// `bpfp`: the queue's `bpf(4)` tap cookie.
    pub bpfp: Cell<*mut *mut u8>,
    // NKSTAT > 0: txkstat; kstat(4) is not configured.
    /// `queue`.
    pub queue: Cell<u32>,
}

impl Vmxnet3Txqueue {
    /// `tq->ifq`.
    fn ifq(&self) -> &'static Ifqueue {
        match self.ifq.get() {
            Some(q) => q,
            None => panic(format_args!("vmx: tx queue without an ifq")),
        }
    }

    /// `tq->ts`.
    fn ts(&self) -> *mut Vmxnet3TxqShared {
        let ts = self.ts.get();
        if ts.is_null() {
            panic(format_args!("vmx: tx queue without its shared structure"));
        }
        ts
    }

    /// `tq->ts->stopped` and `tq->ts->error`.
    fn ts_status(&self) -> (u8, u32) {
        let ts = self.ts();
        // SAFETY: the queue's shared structure in the mapped DMA area (`ts`); the device
        // writes these members, so they are read volatile.
        unsafe {
            (
                ptr::read_volatile(ptr::addr_of!((*ts).stopped)),
                ptr::read_volatile(ptr::addr_of!((*ts).error)),
            )
        }
    }
}

/// `struct vmxnet3_rxqueue`.
#[repr(C, align(64))]
pub struct Vmxnet3Rxqueue {
    /// `sc`: never set, as in C ("sigh").
    pub sc: Cell<*const Vmxnet3Softc>,
    /// `cmd_ring`.
    pub cmd_ring: [Vmxnet3Rxring; 2],
    /// `comp_ring`.
    pub comp_ring: Vmxnet3CompRing,
    /// `rs`: the queue's shared structure, in the DMA area.
    pub rs: Cell<*mut Vmxnet3RxqShared>,
    /// `ifiq`.
    pub ifiq: Cell<Option<&'static Ifiqueue>>,
    // NKSTAT > 0: rxkstat; kstat(4) is not configured.
}

impl Vmxnet3Rxqueue {
    /// `rq->ifiq`.
    fn ifiq(&self) -> &'static Ifiqueue {
        match self.ifiq.get() {
            Some(q) => q,
            None => panic(format_args!("vmx: rx queue without an ifiq")),
        }
    }

    /// `rq->rs`.
    fn rs(&self) -> *mut Vmxnet3RxqShared {
        let rs = self.rs.get();
        if rs.is_null() {
            panic(format_args!("vmx: rx queue without its shared structure"));
        }
        rs
    }

    /// `rq->rs->stopped` and `rq->rs->error`.
    fn rs_status(&self) -> (u8, u32) {
        let rs = self.rs();
        // SAFETY: the queue's shared structure in the mapped DMA area (`rs`); the device
        // writes these members, so they are read volatile.
        unsafe {
            (
                ptr::read_volatile(ptr::addr_of!((*rs).stopped)),
                ptr::read_volatile(ptr::addr_of!((*rs).error)),
            )
        }
    }
}

/// `struct vmxnet3_queue`: a transmit and receive queue pair with its interrupt.
#[repr(C)]
pub struct Vmxnet3Queue {
    /// `tx`.
    pub tx: Vmxnet3Txqueue,
    /// `rx`.
    pub rx: Vmxnet3Rxqueue,
    /// `sc`.
    pub sc: Cell<*const Vmxnet3Softc>,
    /// `intrname`.
    pub intrname: Cell<[u8; 16]>,
    /// `ih`.
    pub ih: Cell<*mut c_void>,
    /// `intr`: the queue's interrupt index (0 without its own vector).
    pub intr: Cell<i32>,
    /// `bpf`: the queue's `bpf(4)` tap.
    pub bpf: Cell<*mut u8>,
}

impl Vmxnet3Queue {
    /// `q->sc`.
    fn sc(&self) -> &'static Vmxnet3Softc {
        let p = self.sc.get();
        if p.is_null() {
            panic(format_args!("vmx: queue without its softc"));
        }
        // SAFETY: vmxnet3_attach points it at the softc that holds the queue; softcs are
        // never freed while their interface exists.
        unsafe { &*p }
    }
}

/// `struct vmxnet3_softc`.
#[repr(C)]
pub struct Vmxnet3Softc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_arpcom`.
    pub sc_arpcom: Arpcom,
    /// `sc_media`.
    pub sc_media: Ifmedia,

    /// `sc_iot0`.
    pub sc_iot0: Cell<Option<BusSpaceTag>>,
    /// `sc_iot1`.
    pub sc_iot1: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh0`.
    pub sc_ioh0: Cell<Option<BusSpaceHandle>>,
    /// `sc_ioh1`.
    pub sc_ioh1: Cell<Option<BusSpaceHandle>>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_ih`.
    pub sc_ih: Cell<*mut c_void>,

    /// `sc_nqueues`.
    pub sc_nqueues: Cell<i32>,
    /// `sc_q`: `sc_nqueues` queues.
    pub sc_q: Cell<*mut Vmxnet3Queue>,
    /// `sc_intrmap`.
    pub sc_intrmap: Cell<Option<&'static Intrmap>>,

    /// `sc_vrrs`.
    pub sc_vrrs: Cell<u32>,
    /// `sc_ds`: the driver-shared structure, in DMA memory.
    pub sc_ds: Cell<*mut Vmxnet3DriverShared>,
    /// `sc_mcast`: the multicast table, in DMA memory.
    pub sc_mcast: Cell<*mut u8>,
    /// `sc_rss`: the RSS configuration, in DMA memory (several queues only).
    pub sc_rss: Cell<*mut Vmxnet3Upt1RssConf>,
    // NKSTAT > 0: sc_kstat_lock, sc_kstat_updated; kstat(4) is not configured.
}

impl Vmxnet3Softc {
    /// `&sc->sc_arpcom.ac_if`.
    pub fn ifp(&'static self) -> &'static Ifnet {
        &self.sc_arpcom.ac_if
    }

    /// `sc->sc_dev.dv_xname`.
    pub fn devname(&self) -> &str {
        self.sc_dev.xname()
    }

    /// `sc->sc_dmat`.
    pub fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => panic(format_args!("{}: no dma tag", self.devname())),
        }
    }

    /// `&sc->sc_q[i]`.
    pub fn q(&self, i: usize) -> &'static Vmxnet3Queue {
        let base = self.sc_q.get();
        if base.is_null() || i >= self.sc_nqueues.get().max(0) as usize {
            panic(format_args!("vmx: no queue {i}"));
        }
        // SAFETY: vmxnet3_attach allocated `sc_nqueues` zeroed queues (valid all-zero) that
        // live as long as the softc.
        unsafe { &*base.add(i) }
    }

    /// The softc's queues, `sc_q[0]` to `sc_q[sc_nqueues - 1]`.
    fn queues(&self) -> impl Iterator<Item = &'static Vmxnet3Queue> + '_ {
        (0..self.sc_nqueues.get().max(0) as usize).map(move |i| self.q(i))
    }

    /// The tag and handle of BAR0 or BAR1.
    fn bar(&self, bar1: bool) -> (BusSpaceTag, BusSpaceHandle) {
        let (t, h) = if bar1 {
            (self.sc_iot1.get(), self.sc_ioh1.get())
        } else {
            (self.sc_iot0.get(), self.sc_ioh0.get())
        };
        match (t, h) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!(
                "{}: BAR{} not mapped",
                self.devname(),
                bar1 as u8
            )),
        }
    }

    /// `READ_BAR0(sc, reg)`.
    pub fn read_bar0(&self, reg: BusSize) -> u32 {
        let (t, h) = self.bar(false);
        bus_space_read_4(t, h, reg)
    }

    /// `READ_BAR1(sc, reg)`.
    pub fn read_bar1(&self, reg: BusSize) -> u32 {
        let (t, h) = self.bar(true);
        bus_space_read_4(t, h, reg)
    }

    /// `WRITE_BAR0(sc, reg, val)`.
    pub fn write_bar0(&self, reg: BusSize, val: u32) {
        let (t, h) = self.bar(false);
        bus_space_write_4(t, h, reg, val)
    }

    /// `WRITE_BAR1(sc, reg, val)`.
    pub fn write_bar1(&self, reg: BusSize, val: u32) {
        let (t, h) = self.bar(true);
        bus_space_write_4(t, h, reg, val)
    }

    /// `WRITE_CMD(sc, cmd)`.
    pub fn write_cmd(&self, cmd: u32) {
        self.write_bar1(VMXNET3_BAR1_CMD, cmd)
    }

    /// `sc->sc_ds`.
    fn ds(&self) -> *mut Vmxnet3DriverShared {
        let ds = self.sc_ds.get();
        if ds.is_null() {
            panic(format_args!("{}: no driver-shared area", self.devname()));
        }
        ds
    }

    /// `sc->sc_ds->event`.
    fn ds_event(&self) -> u32 {
        let ds = self.ds();
        // SAFETY: the driver-shared structure vmxnet3_dma_init mapped (`ds`); the device
        // writes `event`, so it is read volatile.
        unsafe { ptr::read_volatile(ptr::addr_of!((*ds).event)) }
    }

    /// `sc->sc_ds->upt_features`.
    fn ds_upt_features(&self) -> u64 {
        let ds = self.ds();
        // SAFETY: as in `ds_event`.
        unsafe { ptr::read_volatile(ptr::addr_of!((*ds).upt_features)) }
    }

    /// `sc->sc_ds->upt_features = v`.
    fn set_ds_upt_features(&self, v: u64) {
        let ds = self.ds();
        // SAFETY: as in `ds_event`; the device reads the member on the next command.
        unsafe { ptr::write_volatile(ptr::addr_of_mut!((*ds).upt_features), v) }
    }

    /// `sc->sc_ds->ictrl`.
    fn ds_ictrl(&self) -> u32 {
        let ds = self.ds();
        // SAFETY: as in `ds_event`.
        unsafe { ptr::read_volatile(ptr::addr_of!((*ds).ictrl)) }
    }

    /// `sc->sc_ds->ictrl = v`.
    fn set_ds_ictrl(&self, v: u32) {
        let ds = self.ds();
        // SAFETY: as in `set_ds_upt_features`.
        unsafe { ptr::write_volatile(ptr::addr_of_mut!((*ds).ictrl), v) }
    }

    /// `sc->sc_ds->mcast_tablelen = v`.
    fn set_ds_mcast_tablelen(&self, v: u16) {
        let ds = self.ds();
        // SAFETY: as in `set_ds_upt_features`.
        unsafe { ptr::write_volatile(ptr::addr_of_mut!((*ds).mcast_tablelen), v) }
    }

    /// `sc->sc_ds->rxmode = v`.
    fn set_ds_rxmode(&self, v: u32) {
        let ds = self.ds();
        // SAFETY: as in `set_ds_upt_features`.
        unsafe { ptr::write_volatile(ptr::addr_of_mut!((*ds).rxmode), v) }
    }
}

// SAFETY: `#[repr(C)]` with the device first; the arpcom and the ifmedia are all-zero valid
// (`netinet/if_ether.rs`, `net/if_media.rs`), and every other member is a `Cell` of an
// integer, a pointer or an `Option`.
unsafe impl Softc for Vmxnet3Softc {}

/// `vmx_devices[]`.
pub static VMX_DEVICES: [PciMatchid; 1] = [PciMatchid {
    pm_vid: PCI_VENDOR_VMWARE as PciVendorId,
    pm_pid: PCI_PRODUCT_VMWARE_NET_3 as PciProductId,
}];

/// `vmx_ca`.
pub static VMX_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<Vmxnet3Softc>(),
    ca_match: Some(vmxnet3_match),
    ca_attach: vmxnet3_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `vmx_cd`.
pub static VMX_CD: Cfdriver = Cfdriver::new(b"vmx", DV_IFNET, 0);

/// `(struct vmxnet3_softc *)ifp->if_softc`.
pub fn vmxnet3_softc(ifp: &Ifnet) -> &'static Vmxnet3Softc {
    let p = ifp.if_softc.get().cast::<Vmxnet3Softc>().cast_const();
    if p.is_null() {
        panic(format_args!("vmx: interface without its softc"));
    }
    // SAFETY: vmxnet3_attach sets `if_softc` to its softc and installs vmx's functions only
    // on its own interface; softcs outlive their interfaces.
    unsafe { &*p }
}

/// `intr_establish`'s name for a queue's MSI-X vector: a copy of `name`, up to its NUL, that
/// lives as long as the kernel (`evcount(9)` keeps the pointer).
fn vmx_intr_name(name: &[u8]) -> &'static str {
    let n = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let Some(p) = malloc(n.max(1), M_DEVBUF, M_NOWAIT) else {
        return "vmx";
    };
    // SAFETY: a fresh allocation of at least `n` bytes that is never freed, so the copy may
    // be borrowed for the rest of the kernel's life.
    let copy: &'static mut [u8] = unsafe { core::slice::from_raw_parts_mut(p.as_ptr(), n) };
    copy.copy_from_slice(&name[..n]);
    core::str::from_utf8(copy).unwrap_or("vmx")
}

/// `vmxnet3_match`.
pub fn vmxnet3_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    pci_matchbyid(pa, &VMX_DEVICES)
}

/// `vmxnet3_attach`.
pub fn vmxnet3_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    // SAFETY: `self_` was made for `vmx_ca`, whose softc is a `Vmxnet3Softc`; softcs are
    // never freed while their interface exists, so the softc may be borrowed for 'static.
    let sc: &'static Vmxnet3Softc = unsafe { &*ptr::from_ref(self_.softc::<Vmxnet3Softc>()) };
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration
    // of the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let ifp = sc.ifp();

    let memtype = pci_mapreg_type(pa.pa_pc, pa.pa_tag, 0x10);
    let Ok((t, h, _, _)) = pci_mapreg_map(pa, 0x10, memtype, 0, 0) else {
        printf(format_args!(": failed to map BAR0\n"));
        return;
    };
    sc.sc_iot0.set(Some(t));
    sc.sc_ioh0.set(Some(h));
    let memtype = pci_mapreg_type(pa.pa_pc, pa.pa_tag, 0x14);
    let Ok((t, h, _, _)) = pci_mapreg_map(pa, 0x14, memtype, 0, 0) else {
        printf(format_args!(": failed to map BAR1\n"));
        return;
    };
    sc.sc_iot1.set(Some(t));
    sc.sc_ioh1.set(Some(h));

    // Vmxnet3 Revision Report and Selection
    let ver = sc.read_bar1(VMXNET3_BAR1_VRRS);
    if ver & 0x2 != 0 {
        sc.sc_vrrs.set(2);
    } else if ver & 0x1 != 0 {
        sc.sc_vrrs.set(1);
    } else {
        printf(format_args!(": unsupported hardware version 0x{ver:x}\n"));
        return;
    }
    sc.write_bar1(VMXNET3_BAR1_VRRS, sc.sc_vrrs.get());

    // UPT Version Report and Selection
    let ver = sc.read_bar1(VMXNET3_BAR1_UVRS);
    if ver & 0x1 == 0 {
        printf(format_args!(": incompatible UPT version 0x{ver:x}\n"));
        return;
    }
    sc.write_bar1(VMXNET3_BAR1_UVRS, 1);

    sc.sc_dmat.set(Some(pa.pa_dmat));

    sc.write_cmd(VMXNET3_CMD_GET_INTRCFG);
    let intrcfg = sc.read_bar1(VMXNET3_BAR1_CMD);
    let mut isr: PciIntrFn = vmxnet3_intr;
    sc.sc_nqueues.set(1);

    // The C's switch on the interrupt type, with its fall-throughs: MSI-X (or AUTO) when
    // the function has vectors, then MSI, then INTx.
    let itype = intrcfg & VMXNET3_INTRCFG_TYPE_MASK;
    let mut ih: Option<PciIntrHandle> = None;
    if itype == VMXNET3_INTRCFG_TYPE_AUTO || itype == VMXNET3_INTRCFG_TYPE_MSIX {
        let mut msix = pci_intr_msix_count(pa);
        if msix > 0 {
            let Some(h) = pci_intr_map_msix(pa, 0) else {
                printf(format_args!(": failed to map interrupt\n"));
                return;
            };
            ih = Some(h);
            msix -= 1; // are there spares for tx/rx qs?
            if msix != 0 {
                isr = vmxnet3_intr_event;
                let im = intrmap_create(
                    &sc.sc_dev,
                    msix as u32,
                    VMX_MAX_QUEUES.min(IF_MAX_VECTORS) as u32,
                    crate::sys::intrmap::INTRMAP_POWEROF2,
                );
                sc.sc_intrmap.set(Some(im));
                sc.sc_nqueues.set(intrmap_count(im) as i32);
            }
        }
    }
    if ih.is_none() && itype != VMXNET3_INTRCFG_TYPE_INTX {
        ih = pci_intr_map_msi(pa);
    }
    let ih = match ih {
        Some(ih) => ih,
        None => {
            isr = vmxnet3_intr_intx;
            match pci_intr_map(pa) {
                Some(ih) => ih,
                None => {
                    printf(format_args!(": failed to map interrupt\n"));
                    return;
                }
            }
        }
    };
    let intrstr = pci_intr_string(pa.pa_pc, ih);
    let Some(cookie) = pci_intr_establish(
        pa.pa_pc,
        ih,
        IPL_NET | IPL_MPSAFE,
        isr,
        ptr::from_ref(sc).cast_mut().cast(),
        sc.devname(),
    ) else {
        printf(format_args!(
            ": unable to establish interrupt handler at {intrstr}\n"
        ));
        return;
    };
    sc.sc_ih.set(cookie.as_ptr());
    printf(format_args!(": {intrstr}"));

    let nqueues = sc.sc_nqueues.get() as usize;
    let Some(q) = mallocarray(
        nqueues,
        size_of::<Vmxnet3Queue>(),
        M_DEVBUF,
        M_WAITOK | M_ZERO,
    ) else {
        printf(format_args!(", no memory for the queues\n"));
        return;
    };
    if !(q.as_ptr() as usize).is_multiple_of(align_of::<Vmxnet3Queue>()) {
        panic(format_args!("vmx: misaligned queues"));
    }
    sc.sc_q.set(q.as_ptr().cast());

    if let Some(im) = sc.sc_intrmap.get() {
        for i in 0..nqueues {
            let q = sc.q(i);
            let vec = i as i32 + 1;
            let Some(ih) = pci_intr_map_msix(pa, vec) else {
                printf(format_args!(", failed to map interrupt {vec}\n"));
                return;
            };
            let mut name = [0u8; 16];
            let _ = snprintf(&mut name, format_args!("{}:{i}", sc.devname()));
            q.intrname.set(name);
            let Some(qih) = pci_intr_establish_cpu(
                pa.pa_pc,
                ih,
                IPL_NET | IPL_MPSAFE,
                Some(intrmap_cpu(im, i as u32)),
                vmxnet3_intr_queue,
                ptr::from_ref(q).cast_mut().cast(),
                vmx_intr_name(&name),
            ) else {
                printf(format_args!(": unable to establish interrupt {vec}\n"));
                return;
            };
            q.ih.set(qih.as_ptr());

            q.intr.set(vec);
            q.sc.set(sc);
        }
    }

    if vmxnet3_dma_init(sc).is_err() {
        printf(format_args!(": failed to setup DMA\n"));
        return;
    }

    let n = sc.sc_nqueues.get();
    printf(format_args!(", {n} queue{}", if n > 1 { "s" } else { "" }));

    sc.write_cmd(VMXNET3_CMD_GET_MACL);
    let macl = sc.read_bar1(VMXNET3_BAR1_CMD);
    sc.write_cmd(VMXNET3_CMD_GET_MACH);
    let mach = sc.read_bar1(VMXNET3_BAR1_CMD);
    let l = macl.to_le_bytes();
    let hb = mach.to_le_bytes();
    let enaddr: [u8; ETHER_ADDR_LEN] = [l[0], l[1], l[2], l[3], hb[0], hb[1]];

    sc.write_bar1(VMXNET3_BAR1_MACL, macl);
    sc.write_bar1(VMXNET3_BAR1_MACH, mach);
    printf(format_args!(", address {}\n", Str(&ether_sprintf(&enaddr))));

    sc.sc_arpcom.ac_enaddr.set(enaddr);
    let mut xname = [0u8; IFNAMSIZ];
    let name = sc.sc_dev.dv_xname.get();
    let len = name
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(name.len())
        .min(IFNAMSIZ - 1);
    xname[..len].copy_from_slice(&name[..len]);
    ifp.if_xname.set(xname);
    ifp.if_softc.set(ptr::from_ref(sc).cast_mut().cast());
    ifp.if_flags
        .set(IFF_BROADCAST | IFF_MULTICAST | IFF_SIMPLEX);
    ifp.if_xflags.set(IFXF_MPSAFE | IFXF_MBUF_64BIT);
    ifp.if_ioctl.set(Some(vmxnet3_ioctl));
    ifp.if_qstart.set(Some(vmxnet3_start));
    ifp.if_watchdog.set(Some(vmxnet3_watchdog));
    ifp.if_hardmtu.set(VMXNET3_MAX_MTU);
    let mut caps = IFCAP_VLAN_MTU;

    if sc.ds_upt_features() & UPT1_F_CSUM != 0 {
        caps |= IFCAP_CSUM_TCPv4 | IFCAP_CSUM_UDPv4;
        caps |= IFCAP_CSUM_TCPv6 | IFCAP_CSUM_UDPv6;
    }

    caps |= IFCAP_TSOv4 | IFCAP_TSOv6;

    if sc.sc_vrrs.get() == 2 {
        ifp.if_xflags.set(ifp.if_xflags.get() | IFXF_LRO);
        caps |= IFCAP_LRO;
    }

    if NVLAN > 0 && sc.ds_upt_features() & UPT1_F_VLAN != 0 {
        caps |= IFCAP_VLAN_HWTAGGING;
    }
    ifp.if_capabilities.set(caps);

    ifq_init_maxlen(&ifp.if_snd, NTXDESC);

    ifmedia_init(
        &sc.sc_media,
        IFM_IMASK,
        vmxnet3_media_change,
        vmxnet3_media_status,
    );
    ifmedia_add(&sc.sc_media, IFM_ETHER | IFM_AUTO, 0, ptr::null_mut());
    ifmedia_add(
        &sc.sc_media,
        IFM_ETHER | IFM_10G_T | IFM_FDX,
        0,
        ptr::null_mut(),
    );
    ifmedia_add(&sc.sc_media, IFM_ETHER | IFM_10G_T, 0, ptr::null_mut());
    ifmedia_add(
        &sc.sc_media,
        IFM_ETHER | IFM_1000_T | IFM_FDX,
        0,
        ptr::null_mut(),
    );
    ifmedia_add(&sc.sc_media, IFM_ETHER | IFM_1000_T, 0, ptr::null_mut());
    ifmedia_set(&sc.sc_media, IFM_ETHER | IFM_AUTO);

    if_attach(ifp);
    ether_ifattach(&sc.sc_arpcom);
    vmxnet3_link_state(sc);

    if_attach_queues(ifp, nqueues as u32);
    if_attach_iqueues(ifp, nqueues as u32);

    // NKSTAT > 0: vmx_kstat_init(sc); kstat(4) is not configured.

    for i in 0..nqueues {
        let q = sc.q(i);

        let ifq = ifp.ifq(i as u32);
        ifq.ifq_softc.set(ptr::from_ref(&q.tx).cast_mut().cast());
        q.tx.ifq.set(Some(ifq));

        let ifiq = ifp.ifiq(i as u32);
        q.rx.ifiq.set(Some(ifiq));

        // NBPFILTER > 0
        if sc.sc_intrmap.get().is_some() {
            let _ = bpfxattach(
                &q.bpf,
                &q.intrname.get(),
                ifp,
                DLT_EN10MB,
                ETHER_HDR_LEN as u32,
            );

            ifiq.ifiq_bpfp.set(q.bpf.as_ptr());
        }
        q.tx.bpfp.set(q.bpf.as_ptr());

        // NKSTAT > 0: vmx_kstat_txstats(sc, &sc->sc_q[i].tx, i) and
        // vmx_kstat_rxstats(sc, &sc->sc_q[i].rx, i); kstat(4) is not configured.
    }
}

/// `vmxnet3_dma_init`: the queue-shared structures, the rings, the multicast table, the
/// driver-shared structure and (with several queues) the RSS configuration; tells the
/// device where the driver-shared structure is.
pub fn vmxnet3_dma_init(sc: &'static Vmxnet3Softc) -> Result<(), Errno> {
    let nqueues = sc.sc_nqueues.get().max(0) as usize;

    let qs_len = nqueues * (size_of::<Vmxnet3TxqShared>() + size_of::<Vmxnet3RxqShared>());
    let (qs, qs_pa) = vmxnet3_dma_allocmem(sc, qs_len, VMXNET3_DMADESC_ALIGN as BusSize)?;
    let ts = qs.as_ptr().cast::<Vmxnet3TxqShared>();
    for queue in 0..nqueues {
        // SAFETY: the area holds `nqueues` transmit structures, then as many receive ones.
        sc.q(queue).tx.ts.set(unsafe { ts.add(queue) });
    }
    // SAFETY: as above: the receive structures follow the transmit ones.
    let rs = unsafe { ts.add(nqueues) }.cast::<Vmxnet3RxqShared>();
    for queue in 0..nqueues {
        // SAFETY: as above.
        sc.q(queue).rx.rs.set(unsafe { rs.add(queue) });
    }

    for queue in 0..nqueues {
        let intr = sc.q(queue).intr.get();

        vmxnet3_alloc_txring(sc, queue, intr)?;
        vmxnet3_alloc_rxring(sc, queue, intr)?;
    }

    let (mcast, mcast_pa) = vmxnet3_dma_allocmem(sc, VMX_MCAST_LEN, 32)?;
    sc.sc_mcast.set(mcast.as_ptr());

    let (ds, ds_pa) = vmxnet3_dma_allocmem(sc, size_of::<Vmxnet3DriverShared>(), 8)?;
    let ds = ds.as_ptr().cast::<Vmxnet3DriverShared>();
    let mut d = Vmxnet3DriverShared::ZERO;
    d.magic = VMXNET3_REV1_MAGIC;
    d.version = VMXNET3_DRIVER_VERSION;

    // XXX FreeBSD version uses following values:
    // (Does the device behavior depend on them?)
    //
    // major = __FreeBSD_version / 100000;
    // minor = (__FreeBSD_version / 1000) % 100;
    // release_code = (__FreeBSD_version / 100) % 10;
    // rev = __FreeBSD_version % 100;
    let major: u32 = 0;
    let minor: u32 = 0;
    let release_code: u32 = 0;
    let rev: u32 = 0;
    // __LP64__: both architectures are 64-bit.
    d.guest = (release_code << 30)
        | (rev << 22)
        | (major << 14)
        | (minor << 6)
        | VMXNET3_GOS_FREEBSD
        | VMXNET3_GOS_64BIT;
    d.vmxnet3_revision = 1;
    d.upt_version = 1;
    d.upt_features = UPT1_F_CSUM;
    if NVLAN > 0 {
        d.upt_features |= UPT1_F_VLAN;
    }
    d.driver_data = !0u64;
    d.driver_data_len = 0;
    d.queue_shared = qs_pa as u64;
    d.queue_shared_len = qs_len as u32;
    d.mtu = VMXNET3_MAX_MTU;
    d.ntxqueue = nqueues as u8;
    d.nrxqueue = nqueues as u8;
    d.mcast_table = mcast_pa as u64;
    d.automask = 1;
    d.nintr = 1 + if sc.sc_intrmap.get().is_some() {
        nqueues as u8
    } else {
        0
    };
    d.evintr = 0;
    d.ictrl = VMXNET3_ICTRL_DISABLE_ALL;
    for i in 0..usize::from(d.nintr) {
        d.modlevel[i] = UPT1_IMOD_ADAPTIVE;
    }

    if nqueues > 1 {
        let (rss, rss_pa) = vmxnet3_dma_allocmem(sc, size_of::<Vmxnet3Upt1RssConf>(), 8)?;
        let rsscfg = rss.as_ptr().cast::<Vmxnet3Upt1RssConf>();
        let mut r = Vmxnet3Upt1RssConf::ZERO;

        r.hash_type = UPT1_RSS_HASH_TYPE_TCP_IPV4
            | UPT1_RSS_HASH_TYPE_IPV4
            | UPT1_RSS_HASH_TYPE_TCP_IPV6
            | UPT1_RSS_HASH_TYPE_IPV6;
        r.hash_func = UPT1_RSS_HASH_FUNC_TOEPLITZ;
        r.hash_key_size = UPT1_RSS_MAX_KEY_SIZE as u16;
        crate::net::toeplitz::stoeplitz_to_key(&mut r.hash_key);

        r.ind_table_size = UPT1_RSS_MAX_IND_TABLE_SIZE as u16;
        for (i, e) in r.ind_table.iter_mut().enumerate() {
            *e = (i % nqueues) as u8;
        }
        // SAFETY: the area just allocated for it (zeroed, aligned to 8, sized for it), which
        // the device reads only once the driver-shared structure points at it.
        unsafe { ptr::write_volatile(rsscfg, r) };

        d.upt_features |= UPT1_F_RSS;
        d.rss.version = 1;
        d.rss.len = size_of::<Vmxnet3Upt1RssConf>() as u32;
        d.rss.paddr = rss_pa as u64;

        sc.sc_rss.set(rsscfg);
    }

    // SAFETY: the area just allocated for the driver-shared structure (aligned to 8, sized
    // for it); the device does not know it until the BAR1 writes below.
    unsafe { ptr::write_volatile(ds, d) };
    sc.sc_ds.set(ds);

    sc.write_bar1(VMXNET3_BAR1_DSL, ds_pa as u32);
    sc.write_bar1(VMXNET3_BAR1_DSH, (ds_pa as u64 >> 32) as u32);
    Ok(())
}

/// `vmxnet3_alloc_txring`: queue `queue`'s transmit and completion rings, their maps and
/// its shared structure.
pub fn vmxnet3_alloc_txring(sc: &Vmxnet3Softc, queue: usize, intr: i32) -> Result<(), Errno> {
    let tq = &sc.q(queue).tx;
    let ring = &tq.cmd_ring;
    let comp_ring = &tq.comp_ring;

    tq.queue.set(queue as u32);

    vmx_dmamem_alloc(
        sc,
        &ring.dmamem,
        NTXDESC as usize * size_of::<Vmxnet3Txdesc>(),
        512,
    )?;
    ring.txd.set(ring.dmamem.kva().cast());
    vmx_dmamem_alloc(
        sc,
        &comp_ring.dmamem,
        NTXCOMPDESC as usize * size_of::<Vmxnet3Txcompdesc>(),
        512,
    )?;
    comp_ring.txcd.set(comp_ring.dmamem.kva().cast());

    for slot in &ring.dmap {
        let map = bus_dmamap_create(
            sc.dmat(),
            MAXMCLBYTES,
            NTXSEGS as i32,
            VMXNET3_TX_LEN_M as BusSize,
            0,
            BUS_DMA_NOWAIT | BUS_DMA_64BIT,
        )
        .map_err(|_| Errno::ENOMEM)?;
        slot.set(Some(map));
    }

    let mut ts = Vmxnet3TxqShared::ZERO;
    ts.npending = 0;
    ts.intr_threshold = 1;
    ts.cmd_ring = ring.dmamem.dva() as u64;
    ts.cmd_ring_len = NTXDESC;
    ts.comp_ring = comp_ring.dmamem.dva() as u64;
    ts.comp_ring_len = NTXCOMPDESC;
    ts.driver_data = !0u64;
    ts.driver_data_len = 0;
    ts.intr_idx = intr as u8;
    ts.stopped = 1;
    ts.error = 0;
    // SAFETY: the queue's structure in the queue-shared area (`ts()`); the device does not
    // read it before the driver-shared structure points at the area.
    unsafe { ptr::write_volatile(tq.ts(), ts) };
    Ok(())
}

/// `vmxnet3_alloc_rxring`: queue `queue`'s two receive rings, its completion ring, their
/// maps and its shared structure.
pub fn vmxnet3_alloc_rxring(sc: &Vmxnet3Softc, queue: usize, intr: i32) -> Result<(), Errno> {
    let rq = &sc.q(queue).rx;

    for ring in &rq.cmd_ring {
        vmx_dmamem_alloc(
            sc,
            &ring.dmamem,
            NRXDESC as usize * size_of::<Vmxnet3Rxdesc>(),
            512,
        )?;
        ring.rxd.set(ring.dmamem.kva().cast());
    }
    let comp_ring = &rq.comp_ring;
    vmx_dmamem_alloc(
        sc,
        &comp_ring.dmamem,
        NRXCOMPDESC as usize * size_of::<Vmxnet3Rxcompdesc>(),
        512,
    )?;
    comp_ring.rxcd.set(comp_ring.dmamem.kva().cast());

    for (i, ring) in rq.cmd_ring.iter().enumerate() {
        ring.sc.set(sc);
        ring.rid.set(i as u8);
        mtx_init(&ring.mtx, IPL_NET);
        timeout_set(
            &ring.refill,
            vmxnet3_rxfill_tick,
            ptr::from_ref(ring).cast_mut().cast(),
        );
        for slot in &ring.dmap {
            let map = bus_dmamap_create(
                sc.dmat(),
                JUMBO_LEN as BusSize,
                1,
                JUMBO_LEN as BusSize,
                0,
                BUS_DMA_NOWAIT | BUS_DMA_64BIT,
            )
            .map_err(|_| Errno::ENOMEM)?;
            slot.set(Some(map));
        }

        ring.rs.set(rq.rs.get());
        ring.rxh.set(if i == 0 {
            vmxnet3_bar0_rxh1(queue as u32)
        } else {
            vmxnet3_bar0_rxh2(queue as u32)
        });
    }

    let mut rs = Vmxnet3RxqShared::ZERO;
    rs.cmd_ring[0] = rq.cmd_ring[0].dmamem.dva() as u64;
    rs.cmd_ring[1] = rq.cmd_ring[1].dmamem.dva() as u64;
    rs.cmd_ring_len[0] = NRXDESC;
    rs.cmd_ring_len[1] = NRXDESC;
    rs.comp_ring = comp_ring.dmamem.dva() as u64;
    rs.comp_ring_len = NRXCOMPDESC;
    rs.driver_data = !0u64;
    rs.driver_data_len = 0;
    rs.intr_idx = intr as u8;
    rs.stopped = 1;
    rs.error = 0;
    // SAFETY: the queue's structure in the queue-shared area (`rs()`); the device does not
    // read it before the driver-shared structure points at the area.
    unsafe { ptr::write_volatile(rq.rs(), rs) };
    Ok(())
}

/// Zeroes `n` descriptors of type `T` at `base`, a ring's DMA memory (the C's `memset` of
/// its `dmamem`), volatile.
fn vmx_ring_zero<T: Default>(base: *mut T, n: u32) {
    if base.is_null() {
        panic(format_args!("vmx: zeroing an unmapped ring"));
    }
    for i in 0..n as usize {
        // SAFETY: the ring's allocation holds `n` descriptors at `base`; the device does not
        // use the ring while it is reinitialised.
        unsafe { ptr::write_volatile(base.add(i), T::default()) };
    }
}

/// `vmxnet3_txinit`.
pub fn vmxnet3_txinit(sc: &Vmxnet3Softc, tq: &Vmxnet3Txqueue) {
    let ring = &tq.cmd_ring;
    let comp_ring = &tq.comp_ring;
    let dmat = sc.dmat();

    ring.prod.store(0, Ordering::Relaxed);
    ring.cons.store(0, Ordering::Relaxed);
    ring.r#gen.set(VMX_TX_GEN);
    comp_ring.next.set(0);
    comp_ring.r#gen.set(VMX_TXC_GEN);
    vmx_ring_zero(ring.txd.get(), NTXDESC);
    bus_dmamap_sync(
        dmat,
        ring.dmamem.map(),
        0,
        ring.dmamem.len(),
        BUS_DMASYNC_PREWRITE,
    );
    vmx_ring_zero(comp_ring.txcd.get(), NTXCOMPDESC);
    bus_dmamap_sync(
        dmat,
        comp_ring.dmamem.map(),
        0,
        comp_ring.dmamem.len(),
        BUS_DMASYNC_PREREAD,
    );

    ifq_clr_oactive(tq.ifq());
}

/// `vmxnet3_rxfill_tick`: the refill timeout of an empty ring.
pub fn vmxnet3_rxfill_tick(arg: *mut c_void) {
    // SAFETY: vmxnet3_alloc_rxring set the timeout with the ring as its argument; the ring
    // lives as long as the softc.
    let ring = unsafe { &*arg.cast::<Vmxnet3Rxring>().cast_const() };

    if !mtx_enter_try(&ring.mtx) {
        return;
    }

    vmxnet3_rxfill(ring);
    mtx_leave(&ring.mtx);
}

/// `vmxnet3_rxfill`: hands clusters to the device in the ring's free slots.
pub fn vmxnet3_rxfill(ring: &Vmxnet3Rxring) {
    let sc = ring.sc();
    let dmat = sc.dmat();
    let mut type_ = (VMXNET3_BTYPE_HEAD << VMXNET3_RX_BTYPE_S).to_le();

    // Second ring just contains packet bodies.
    if ring.rid.get() == 1 {
        type_ = (VMXNET3_BTYPE_BODY << VMXNET3_RX_BTYPE_S).to_le();
    }

    mutex_assert_locked(&ring.mtx, "vmxnet3_rxfill");

    let mut slots = ring.with_rxr(|r| if_rxr_get(r, NRXDESC));
    if slots == 0 {
        return;
    }

    let mut prod = ring.fill.get();
    let mut rgen = ring.r#gen.get();

    let rmap = ring.dmamem.map();
    bus_dmamap_sync(dmat, rmap, 0, ring.dmamem.len(), BUS_DMASYNC_POSTWRITE);

    loop {
        kassert!(ring.m[prod as usize].get().is_none());

        let Some(m) = m_clget(None, M_DONTWAIT, JUMBO_LEN) else {
            break;
        };

        m.m_len().set(JUMBO_LEN);
        m.m_pkthdr().len.set(JUMBO_LEN as i32);
        m_adj(m, ETHER_ALIGN as i32);

        let map = ring.map(prod);
        // SAFETY: the cluster stays the slot's (`ring->m[prod]`) until vmxnet3_rxintr or
        // vmxnet3_rxstop unload the map before handing it on or freeing it.
        if unsafe { bus_dmamap_load_mbuf(dmat, map, m, BUS_DMA_NOWAIT) }.is_err() {
            panic(format_args!("load mbuf"));
        }

        bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_PREREAD);

        ring.m[prod as usize].set(Some(m));

        let mut rxd = ring.rxd_get(prod);
        rxd.rx_addr = (map.dm_segs()[0].get().ds_addr as u64).to_le();
        ring.rxd_set(prod, rxd);
        bus_dmamap_sync(
            dmat,
            rmap,
            0,
            ring.dmamem.len(),
            BUS_DMASYNC_PREWRITE | BUS_DMASYNC_POSTWRITE,
        );
        rxd.rx_word2 = ((m.m_pkthdr().len.get() as u32 & VMXNET3_RX_LEN_M).to_le()
            << VMXNET3_RX_LEN_S)
            | type_
            | rgen;
        ring.rxd_set(prod, rxd);

        prod += 1;
        if prod == NRXDESC {
            prod = 0;
            rgen ^= VMX_RX_GEN;
        }

        slots -= 1;
        if slots == 0 {
            break;
        }
    }

    bus_dmamap_sync(dmat, rmap, 0, ring.dmamem.len(), BUS_DMASYNC_PREWRITE);

    ring.with_rxr(|r| if_rxr_put(r, slots));

    ring.fill.set(prod);
    ring.r#gen.set(rgen);

    if if_rxr_inuse(&ring.rxr.get()) == 0 {
        timeout_add(&ring.refill, 1);
    }

    if ring.rs_update_rxhead() != 0 {
        sc.write_bar0(ring.rxh.get(), prod);
    }
}

/// `vmxnet3_rxinit`.
pub fn vmxnet3_rxinit(sc: &Vmxnet3Softc, rq: &Vmxnet3Rxqueue) {
    let dmat = sc.dmat();

    for ring in &rq.cmd_ring {
        ring.with_rxr(|r| if_rxr_init(r, 2, NRXDESC - 1));
        ring.fill.set(0);
        ring.r#gen.set(VMX_RX_GEN);

        vmx_ring_zero(ring.rxd.get(), NRXDESC);
        bus_dmamap_sync(
            dmat,
            ring.dmamem.map(),
            0,
            ring.dmamem.len(),
            BUS_DMASYNC_PREWRITE,
        );

        mtx_enter(&ring.mtx);
        vmxnet3_rxfill(ring);
        mtx_leave(&ring.mtx);
    }

    let comp_ring = &rq.comp_ring;
    comp_ring.next.set(0);
    comp_ring.r#gen.set(VMX_RXC_GEN);
    comp_ring.sendmp.set(None);
    comp_ring.lastmp.set(None);

    vmx_ring_zero(comp_ring.rxcd.get(), NRXCOMPDESC);
    bus_dmamap_sync(
        dmat,
        comp_ring.dmamem.map(),
        0,
        comp_ring.dmamem.len(),
        BUS_DMASYNC_PREREAD,
    );
}

/// `vmxnet3_txstop`.
pub fn vmxnet3_txstop(sc: &Vmxnet3Softc, tq: &Vmxnet3Txqueue) {
    let ring = &tq.cmd_ring;
    let comp_ring = &tq.comp_ring;
    let ifq = tq.ifq();
    let dmat = sc.dmat();

    bus_dmamap_sync(
        dmat,
        comp_ring.dmamem.map(),
        0,
        comp_ring.dmamem.len(),
        BUS_DMASYNC_POSTREAD,
    );
    bus_dmamap_sync(
        dmat,
        ring.dmamem.map(),
        0,
        ring.dmamem.len(),
        BUS_DMASYNC_POSTWRITE,
    );

    for idx in 0..NTXDESC {
        if let Some(m) = ring.m[idx as usize].take() {
            bus_dmamap_unload(dmat, ring.map(idx));
            m_freem(m);
        }
    }

    ifq_purge(ifq);
    ifq_clr_oactive(ifq);
}

/// `vmxnet3_rxstop`.
pub fn vmxnet3_rxstop(sc: &Vmxnet3Softc, rq: &Vmxnet3Rxqueue) {
    let comp_ring = &rq.comp_ring;
    let dmat = sc.dmat();

    bus_dmamap_sync(
        dmat,
        comp_ring.dmamem.map(),
        0,
        comp_ring.dmamem.len(),
        BUS_DMASYNC_POSTREAD,
    );

    for ring in &rq.cmd_ring {
        bus_dmamap_sync(
            dmat,
            ring.dmamem.map(),
            0,
            ring.dmamem.len(),
            BUS_DMASYNC_POSTWRITE,
        );
        timeout_del(&ring.refill);
        for idx in 0..NRXDESC {
            let Some(m) = ring.m[idx as usize].take() else {
                continue;
            };

            // Unloaded before the free: the map's buffer stays allocated until then.
            bus_dmamap_unload(dmat, ring.map(idx));
            m_freem(m);
        }
    }
}

/// `vmxnet3_link_state`: asks the device for the link and reports a change.
pub fn vmxnet3_link_state(sc: &'static Vmxnet3Softc) {
    let ifp = sc.ifp();

    sc.write_cmd(VMXNET3_CMD_GET_LINK);
    let x = sc.read_bar1(VMXNET3_BAR1_CMD);
    let speed = x >> 16;
    let link = if x & 1 != 0 {
        ifp.if_baudrate.set(if_mbps(u64::from(speed)));
        LINK_STATE_UP
    } else {
        LINK_STATE_DOWN
    };

    if ifp.if_link_state.get() != link {
        ifp.if_link_state.set(link);
        if_link_state_change(ifp);
    }
}

/// `vmxnet3_enable_intr`.
#[inline]
fn vmxnet3_enable_intr(sc: &Vmxnet3Softc, irq: i32) {
    sc.write_bar0(vmxnet3_bar0_imask(irq as u32), 0);
}

/// `vmxnet3_disable_intr`.
#[inline]
fn vmxnet3_disable_intr(sc: &Vmxnet3Softc, irq: i32) {
    sc.write_bar0(vmxnet3_bar0_imask(irq as u32), 1);
}

/// `vmxnet3_enable_all_intrs`.
pub fn vmxnet3_enable_all_intrs(sc: &Vmxnet3Softc) {
    sc.set_ds_ictrl(sc.ds_ictrl() & !VMXNET3_ICTRL_DISABLE_ALL);
    vmxnet3_enable_intr(sc, 0);
    if sc.sc_intrmap.get().is_some() {
        for q in sc.queues() {
            vmxnet3_enable_intr(sc, q.intr.get());
        }
    }
}

/// `vmxnet3_disable_all_intrs`.
pub fn vmxnet3_disable_all_intrs(sc: &Vmxnet3Softc) {
    sc.set_ds_ictrl(sc.ds_ictrl() | VMXNET3_ICTRL_DISABLE_ALL);
    vmxnet3_disable_intr(sc, 0);
    if sc.sc_intrmap.get().is_some() {
        for q in sc.queues() {
            vmxnet3_disable_intr(sc, q.intr.get());
        }
    }
}

/// `vmxnet3_intr_intx`: INTx, which other functions may share.
pub fn vmxnet3_intr_intx(arg: *mut c_void) -> i32 {
    // SAFETY: vmxnet3_attach established the handler with the softc as its argument.
    let sc = unsafe { &*arg.cast::<Vmxnet3Softc>().cast_const() };

    if sc.read_bar1(VMXNET3_BAR1_INTR) == 0 {
        return 0;
    }

    vmxnet3_intr(arg)
}

/// `vmxnet3_intr`: the one interrupt of MSI or a lone MSI-X vector: events and queue 0.
pub fn vmxnet3_intr(arg: *mut c_void) -> i32 {
    // SAFETY: vmxnet3_attach established the handler with the softc as its argument.
    let sc: &'static Vmxnet3Softc = unsafe { &*arg.cast::<Vmxnet3Softc>().cast_const() };
    let ifp = sc.ifp();

    if sc.ds_event() != 0 {
        kernel_lock();
        vmxnet3_evintr(sc);
        kernel_unlock();
    }

    if ifp.if_flags.get() & IFF_RUNNING != 0 {
        vmxnet3_rxintr(sc, &sc.q(0).rx);
        vmxnet3_txintr(sc, &sc.q(0).tx);
        vmxnet3_enable_intr(sc, 0);
    }

    1
}

/// `vmxnet3_intr_event`: MSI-X vector 0 when the queues have their own.
pub fn vmxnet3_intr_event(arg: *mut c_void) -> i32 {
    // SAFETY: vmxnet3_attach established the handler with the softc as its argument.
    let sc: &'static Vmxnet3Softc = unsafe { &*arg.cast::<Vmxnet3Softc>().cast_const() };

    if sc.ds_event() != 0 {
        kernel_lock();
        vmxnet3_evintr(sc);
        kernel_unlock();
    }

    vmxnet3_enable_intr(sc, 0);
    1
}

/// `vmxnet3_intr_queue`: a queue's MSI-X vector.
pub fn vmxnet3_intr_queue(arg: *mut c_void) -> i32 {
    // SAFETY: vmxnet3_attach established the handler with one of the softc's queues as its
    // argument; the queues live as long as the softc.
    let q = unsafe { &*arg.cast::<Vmxnet3Queue>().cast_const() };
    let sc = q.sc();

    vmxnet3_rxintr(sc, &q.rx);
    vmxnet3_txintr(sc, &q.tx);
    vmxnet3_enable_intr(sc, q.intr.get());

    1
}

/// `vmxnet3_evintr`: the device's events (link, queue errors, ...). Under the kernel lock.
pub fn vmxnet3_evintr(sc: &'static Vmxnet3Softc) {
    let ifp = sc.ifp();
    let event = sc.ds_event();

    // Clear events.
    sc.write_bar1(VMXNET3_BAR1_EVENT, event);

    // Link state change?
    if event & VMXNET3_EVENT_LINK != 0 {
        vmxnet3_link_state(sc);
    }

    // Queue error?
    if event & (VMXNET3_EVENT_TQERROR | VMXNET3_EVENT_RQERROR) != 0 {
        sc.write_cmd(VMXNET3_CMD_GET_STATUS);

        let (stopped, error) = sc.q(0).tx.ts_status();
        if stopped != 0 {
            printf(format_args!(
                "{}: TX error 0x{error:x}\n",
                Str(&ifp.if_xname.get())
            ));
        }
        let (stopped, error) = sc.q(0).rx.rs_status();
        if stopped != 0 {
            printf(format_args!(
                "{}: RX error 0x{error:x}\n",
                Str(&ifp.if_xname.get())
            ));
        }
        let _ = vmxnet3_init(sc);
    }

    if event & VMXNET3_EVENT_DIC != 0 {
        printf(format_args!(
            "{}: device implementation change event\n",
            Str(&ifp.if_xname.get())
        ));
    }
    if event & VMXNET3_EVENT_DEBUG != 0 {
        printf(format_args!("{}: debug event\n", Str(&ifp.if_xname.get())));
    }
}

/// `vmxnet3_txintr`: frees what the device sent.
pub fn vmxnet3_txintr(sc: &Vmxnet3Softc, tq: &Vmxnet3Txqueue) {
    let ifq = tq.ifq();
    let ring = &tq.cmd_ring;
    let comp_ring = &tq.comp_ring;
    let dmat = sc.dmat();
    let mut done = false;

    let prod = ring.prod.load(Ordering::Acquire);
    let mut cons = ring.cons.load(Ordering::Relaxed);

    if cons == prod {
        return;
    }

    let mut next = comp_ring.next.get();
    let mut rgen = comp_ring.r#gen.get();

    let cmap = comp_ring.dmamem.map();
    bus_dmamap_sync(dmat, cmap, 0, comp_ring.dmamem.len(), BUS_DMASYNC_POSTREAD);

    loop {
        if comp_ring.txcd_word3(next) & VMX_TXC_GEN != rgen {
            break;
        }
        let txcd = comp_ring.txcd_get(next);

        next += 1;
        if next == NTXCOMPDESC {
            next = 0;
            rgen ^= VMX_TXC_GEN;
        }

        let m = ring.m[cons as usize].take();

        kassert!(m.is_some());

        let map = ring.map(cons);
        bus_dmamap_unload(dmat, map);
        m_freem(m);

        cons = (u32::from_le(txcd.txc_word0) >> VMXNET3_TXC_EOPIDX_S) & VMXNET3_TXC_EOPIDX_M;
        cons += 1;
        done = true;
        cons %= NTXDESC;
        if cons == prod {
            break;
        }
    }

    bus_dmamap_sync(dmat, cmap, 0, comp_ring.dmamem.len(), BUS_DMASYNC_PREREAD);

    comp_ring.next.set(next);
    comp_ring.r#gen.set(rgen);
    ring.cons.store(cons, Ordering::Release);

    if done && ifq_is_oactive(ifq) {
        ifq_restart(ifq);
    }
}

/// `vmxnet3_rxintr`: passes up what the device received and refills the rings.
pub fn vmxnet3_rxintr(sc: &'static Vmxnet3Softc, rq: &Vmxnet3Rxqueue) {
    let ifp = sc.ifp();
    let comp_ring = &rq.comp_ring;
    let dmat = sc.dmat();
    let ml = MbufList::new();
    let mut done = [0u32; 2];

    let mut next = comp_ring.next.get();
    let mut rgen = comp_ring.r#gen.get();

    let cmap = comp_ring.dmamem.map();
    bus_dmamap_sync(dmat, cmap, 0, comp_ring.dmamem.len(), BUS_DMASYNC_POSTREAD);

    loop {
        if comp_ring.rxcd_word3(next) & VMX_RXC_GEN != rgen {
            break;
        }
        let rxcd = comp_ring.rxcd_get(next);

        next += 1;
        if next == NRXCOMPDESC {
            next = 0;
            rgen ^= VMX_RXC_GEN;
        }

        let word0 = u32::from_le(rxcd.rxc_word0);
        let idx = (word0 >> VMXNET3_RXC_IDX_S) & VMXNET3_RXC_IDX_M;

        let rid = if ((word0 >> VMXNET3_RXC_QID_S) & VMXNET3_RXC_QID_M) < sc.sc_nqueues.get() as u32
        {
            0
        } else {
            1
        };

        let ring = &rq.cmd_ring[rid];

        let Some(m) = ring.m[idx as usize].take() else {
            panic(format_args!(
                "{}: no mbuf in rx ring {rid} slot {idx}",
                sc.devname()
            ));
        };

        let map = ring.map(idx);
        bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTREAD);
        bus_dmamap_unload(dmat, map);

        done[rid] += 1;

        // A receive descriptor of type 4 which is flagged as start of packet, contains the
        // number of TCP segment of an LRO packet.
        if (u32::from_le(rxcd.rxc_word3) & VMXNET3_RXC_TYPE_M) >> VMXNET3_RXC_TYPE_S == 4
            && word0 & VMXNET3_RXC_SOP != 0
        {
            m.m_pkthdr()
                .ph_mss
                .set((u32::from_le(rxcd.rxc_word1) & VMXNET3_RXC_SEG_CNT_M) as u16);
        }

        m.m_len()
            .set((u32::from_le(rxcd.rxc_word2) >> VMXNET3_RXC_LEN_S) & VMXNET3_RXC_LEN_M);

        let sendmp = match (comp_ring.sendmp.get(), comp_ring.lastmp.get()) {
            (Some(sendmp), Some(lastmp)) => {
                m.m_flags().set(m.m_flags().get() & !M_PKTHDR);
                lastmp.m_next().set(Some(m));
                comp_ring.lastmp.set(Some(m));
                sendmp
            }
            _ => {
                comp_ring.sendmp.set(Some(m));
                comp_ring.lastmp.set(Some(m));
                m.m_pkthdr().len.set(0);
                m
            }
        };
        sendmp
            .m_pkthdr()
            .len
            .set(sendmp.m_pkthdr().len.get() + m.m_len().get() as i32);

        if word0 & VMXNET3_RXC_EOP == 0 {
            continue;
        }

        // End of Packet

        if u32::from_le(rxcd.rxc_word2) & VMXNET3_RXC_ERROR != 0 {
            ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
            m_freem(sendmp);
            comp_ring.sendmp.set(None);
            comp_ring.lastmp.set(None);
            continue;
        }

        if (sendmp.m_pkthdr().len.get() as u32) < VMXNET3_MIN_MTU {
            m_freem(sendmp);
            comp_ring.sendmp.set(None);
            comp_ring.lastmp.set(None);
            continue;
        }

        if (word0 >> VMXNET3_RXC_RSSTYPE_S) & VMXNET3_RXC_RSSTYPE_M != VMXNET3_RXC_RSSTYPE_NONE {
            let ph = sendmp.m_pkthdr();
            ph.ph_flowid.set(u32::from_le(rxcd.rxc_word1) as u16);
            ph.csum_flags.set(ph.csum_flags.get() | M_FLOWID);
        }

        vmxnet3_rx_offload(&rxcd, sendmp);
        ml_enqueue(&ml, sendmp);
        comp_ring.sendmp.set(None);
        comp_ring.lastmp.set(None);
    }

    bus_dmamap_sync(dmat, cmap, 0, comp_ring.dmamem.len(), BUS_DMASYNC_PREREAD);

    comp_ring.next.set(next);
    comp_ring.r#gen.set(rgen);

    for (i, ring) in rq.cmd_ring.iter().enumerate() {
        if done[i] == 0 {
            continue;
        }

        let livelocked = ifiq_input(rq.ifiq(), &ml);

        mtx_enter(&ring.mtx);
        if livelocked {
            ring.with_rxr(if_rxr_livelocked);
        }
        ring.with_rxr(|r| if_rxr_put(r, done[i]));
        vmxnet3_rxfill(ring);
        mtx_leave(&ring.mtx);
    }
}

/// `vmxnet3_iff`: programs the receive mode and the multicast table.
pub fn vmxnet3_iff(sc: &'static Vmxnet3Softc) {
    let ifp = sc.ifp();
    let ac = &sc.sc_arpcom;

    sc.set_ds_mcast_tablelen(0);
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_ALLMULTI);

    // Always accept broadcast frames.
    // Always accept frames destined to our station address.
    let mut mode = VMXNET3_RXMODE_BCAST | VMXNET3_RXMODE_UCAST;

    if ifp.if_flags.get() & IFF_PROMISC != 0
        || ac.ac_multirangecnt.get() > 0
        || ac.ac_multicnt.get() > 682
    {
        ifp.if_flags.set(ifp.if_flags.get() | IFF_ALLMULTI);
        mode |= VMXNET3_RXMODE_ALLMULTI | VMXNET3_RXMODE_MCAST;
        if ifp.if_flags.get() & IFF_PROMISC != 0 {
            mode |= VMXNET3_RXMODE_PROMISC;
        }
    } else {
        let base = sc.sc_mcast.get();
        if base.is_null() {
            panic(format_args!("{}: no multicast table", sc.devname()));
        }
        let mut p = 0usize;
        let mut step = EtherMultistep { e_enm: None };
        let mut enm = ether_first_multi(&mut step, ac);
        while let Some(e) = enm {
            if p + ETHER_ADDR_LEN > VMX_MCAST_LEN {
                break;
            }
            for (k, &b) in e.enm_addrlo.iter().enumerate() {
                // SAFETY: the table vmxnet3_dma_init mapped holds VMX_MCAST_LEN bytes, and
                // `p + k` is below that (checked above).
                unsafe { ptr::write_volatile(base.add(p + k), b) };
            }

            p += ETHER_ADDR_LEN;

            enm = ether_next_multi(&mut step);
        }

        if ac.ac_multicnt.get() > 0 {
            mode |= VMXNET3_RXMODE_MCAST;
            sc.set_ds_mcast_tablelen(p as u16);
        }
    }

    sc.write_cmd(VMXNET3_CMD_SET_FILTER);
    sc.set_ds_rxmode(mode);
    sc.write_cmd(VMXNET3_CMD_SET_RXMODE);
}

/// `vmxnet3_rx_offload`: VLAN, checksum and LRO information of a received packet.
pub fn vmxnet3_rx_offload(rxcd: &Vmxnet3Rxcompdesc, m: &Mbuf) {
    let ph = m.m_pkthdr();
    let word0 = u32::from_le(rxcd.rxc_word0);
    let word2 = u32::from_le(rxcd.rxc_word2);
    let word3 = u32::from_le(rxcd.rxc_word3);

    // VLAN Offload

    if NVLAN > 0 && word2 & VMXNET3_RXC_VLAN != 0 {
        m.m_flags().set(m.m_flags().get() | M_VLANTAG);
        ph.ether_vtag
            .set(((word2 >> VMXNET3_RXC_VLANTAG_S) & VMXNET3_RXC_VLANTAG_M) as u16);
    }

    // Checksum Offload

    if word0 & VMXNET3_RXC_NOCSUM != 0 {
        return;
    }

    if word3 & VMXNET3_RXC_IPV4 != 0 && word3 & VMXNET3_RXC_IPSUM_OK != 0 {
        ph.csum_flags.set(ph.csum_flags.get() | M_IPV4_CSUM_IN_OK);
    }

    if word3 & VMXNET3_RXC_FRAGMENT != 0 {
        return;
    }

    if word3 & VMXNET3_RXC_CSUM_OK != 0 {
        if word3 & VMXNET3_RXC_TCP != 0 {
            ph.csum_flags.set(ph.csum_flags.get() | M_TCP_CSUM_IN_OK);
        } else if word3 & VMXNET3_RXC_UDP != 0 {
            ph.csum_flags.set(ph.csum_flags.get() | M_UDP_CSUM_IN_OK);
        }
    }

    // TCP Large Receive Offload

    let pkts = u32::from(ph.ph_mss.get());
    ph.ph_mss.set(0);

    if pkts > 1 {
        let mut ext = EtherExtracted::new();

        ether_extract_headers(m, &mut ext);

        let mut paylen = ext.iplen;
        if !ext.ip4.is_null() || !ext.ip6.is_null() {
            paylen = paylen.wrapping_sub(ext.iphlen);
        }

        if !ext.tcp.is_null() {
            paylen = paylen.wrapping_sub(ext.tcphlen);
            tcpstat_inc(TcpstatCounters::TcpsInhwlro);
            tcpstat_add(TcpstatCounters::TcpsInpktlro, u64::from(pkts));
        } else {
            tcpstat_inc(TcpstatCounters::TcpsInbadlro);
        }

        // If we gonna forward this packet, we have to mark it as TSO, set a correct mss,
        // and recalculate the TCP checksum.
        if !ext.tcp.is_null() && paylen >= pkts {
            ph.csum_flags.set(ph.csum_flags.get() | M_TCP_TSO);
            ph.ph_mss.set((paylen / pkts) as u16);
        }
        if !ext.tcp.is_null() && ph.csum_flags.get() & M_TCP_CSUM_IN_OK != 0 {
            ph.csum_flags.set(ph.csum_flags.get() | M_TCP_CSUM_OUT);
        }
    }
}

/// `vmxnet3_stop`.
pub fn vmxnet3_stop(ifp: &'static Ifnet) {
    let sc = vmxnet3_softc(ifp);

    ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);
    ifp.if_timer.set(0);

    vmxnet3_disable_all_intrs(sc);

    sc.write_cmd(VMXNET3_CMD_DISABLE);

    if sc.sc_intrmap.get().is_some() {
        for q in sc.queues() {
            if let Some(ih) = NonNull::new(q.ih.get()) {
                intr_barrier(ih);
            }
        }
    } else if let Some(ih) = NonNull::new(sc.sc_ih.get()) {
        intr_barrier(ih);
    }

    for q in sc.queues() {
        vmxnet3_txstop(sc, &q.tx);
    }
    for q in sc.queues() {
        vmxnet3_rxstop(sc, &q.rx);
    }
}

/// `vmxnet3_reset`.
pub fn vmxnet3_reset(sc: &Vmxnet3Softc) {
    sc.write_cmd(VMXNET3_CMD_RESET);
}

/// `vmxnet4_set_features`: tells the device whether to do LRO (the C's name).
pub fn vmxnet4_set_features(sc: &'static Vmxnet3Softc) {
    let ifp = sc.ifp();

    // TCP Large Receive Offload
    if ifp.if_xflags.get() & IFXF_LRO != 0 {
        sc.set_ds_upt_features(sc.ds_upt_features() | UPT1_F_LRO);
    } else {
        sc.set_ds_upt_features(sc.ds_upt_features() & !UPT1_F_LRO);
    }
    sc.write_cmd(VMXNET3_CMD_SET_FEATURE);
}

/// `vmxnet3_init`.
pub fn vmxnet3_init(sc: &'static Vmxnet3Softc) -> Result<(), Errno> {
    let ifp = sc.ifp();

    // Cancel pending I/O and free all RX/TX buffers.
    vmxnet3_stop(ifp);

    // #if 0: Put controller into known state (vmxnet3_reset(sc)); not compiled in C.

    for q in sc.queues() {
        vmxnet3_txinit(sc, &q.tx);
    }
    for q in sc.queues() {
        vmxnet3_rxinit(sc, &q.rx);
    }

    for queue in 0..sc.sc_nqueues.get().max(0) as u32 {
        sc.write_bar0(vmxnet3_bar0_rxh1(queue), 0);
        sc.write_bar0(vmxnet3_bar0_rxh2(queue), 0);
    }

    sc.write_cmd(VMXNET3_CMD_ENABLE);
    if sc.read_bar1(VMXNET3_BAR1_CMD) != 0 {
        printf(format_args!(
            "{}: failed to initialize\n",
            Str(&ifp.if_xname.get())
        ));
        vmxnet3_stop(ifp);
        return Err(Errno::EIO);
    }

    vmxnet4_set_features(sc);

    // Program promiscuous mode and multicast filters.
    vmxnet3_iff(sc);

    vmxnet3_enable_all_intrs(sc);

    vmxnet3_link_state(sc);

    ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);

    Ok(())
}

/// `vmx_rxr_info`: `SIOCGIFRXR`, the first receive ring of each queue.
fn vmx_rxr_info(sc: &Vmxnet3Softc, ifri: usize) -> Result<(), Errno> {
    let n = sc.sc_nqueues.get().max(0) as usize;
    let Some(p) = mallocarray(
        n,
        size_of::<IfRxringInfo>(),
        M_TEMP,
        M_WAITOK | M_ZERO | M_CANFAIL,
    ) else {
        return Err(Errno::ENOMEM);
    };
    // SAFETY: a fresh zeroed array of `n` entries (valid all-zero), freed below.
    let ifrs = unsafe { core::slice::from_raw_parts_mut(p.as_ptr().cast::<IfRxringInfo>(), n) };

    for (i, ifr) in ifrs.iter_mut().enumerate() {
        ifr.ifr_size = JUMBO_LEN;
        let _ = snprintf(&mut ifr.ifr_name, format_args!("{i}"));
        ifr.ifr_info = sc.q(i).rx.cmd_ring[0].rxr.get();
    }

    let error = if_rxr_info_ioctl(ifri, n as u32, ifrs);

    free(p, M_TEMP, n * size_of::<IfRxringInfo>());

    error
}

/// `vmxnet3_ioctl`.
///
/// # Safety
///
/// As for `IfIoctlFn` (`net/if_var.rs`): `data` is the kernel copy of the request the
/// command encodes.
pub unsafe fn vmxnet3_ioctl(ifp: &'static Ifnet, cmd: u64, data: *mut u8) -> Result<(), Errno> {
    let sc = vmxnet3_softc(ifp);
    let mut error: Result<(), Errno> = Ok(());

    let s = splnet();

    match cmd {
        SIOCSIFADDR => {
            ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
            if ifp.if_flags.get() & IFF_RUNNING == 0 {
                error = vmxnet3_init(sc);
            }
        }
        SIOCSIFFLAGS => {
            if ifp.if_flags.get() & IFF_UP != 0 {
                if ifp.if_flags.get() & IFF_RUNNING != 0 {
                    error = Err(Errno::ENETRESET);
                } else {
                    error = vmxnet3_init(sc);
                }
            } else if ifp.if_flags.get() & IFF_RUNNING != 0 {
                vmxnet3_stop(ifp);
            }
        }
        SIOCSIFXFLAGS => {
            // SAFETY: SIOCSIFXFLAGS carries a `struct ifreq` (this function's contract).
            let ifr = unsafe { &*data.cast::<Ifreq>() };
            let flags = i32::from(ifr.ifr_flags());
            if flags & IFXF_LRO != ifp.if_xflags.get() & IFXF_LRO {
                if flags & IFXF_LRO != 0 {
                    ifp.if_xflags.set(ifp.if_xflags.get() | IFXF_LRO);
                } else {
                    ifp.if_xflags.set(ifp.if_xflags.get() & !IFXF_LRO);
                }

                vmxnet4_set_features(sc);
            }
        }
        SIOCSIFMEDIA | SIOCGIFMEDIA => {
            // SAFETY: SIOCSIFMEDIA carries a `struct ifreq` and SIOCGIFMEDIA a `struct
            // ifmediareq` (this function's contract).
            error = unsafe { ifmedia_ioctl(ifp, data, &sc.sc_media, cmd) };
        }
        SIOCGIFRXR => {
            // SAFETY: SIOCGIFRXR carries a `struct ifreq` (this function's contract).
            let ifr = unsafe { &*data.cast::<Ifreq>() };
            error = vmx_rxr_info(sc, ifr.ifr_data() as usize);
        }
        _ => {
            // SAFETY: the caller's contract, forwarded.
            error = unsafe { ether_ioctl(ifp, &sc.sc_arpcom, cmd, data) };
        }
    }

    if error == Err(Errno::ENETRESET) {
        if ifp.if_flags.get() & IFF_RUNNING != 0 {
            vmxnet3_iff(sc);
        }
        error = Ok(());
    }

    splx(s);
    error
}

/// `vmx_load_mbuf`: loads `m`, defragmenting it once when it has too many segments.
#[inline]
fn vmx_load_mbuf(dmat: BusDmaTag, map: &BusDmamap, m: &'static Mbuf) -> Result<(), Errno> {
    // SAFETY: the mbuf stays the slot's (`ring->m[prod]`) until vmxnet3_txintr or
    // vmxnet3_txstop unload the map before freeing it.
    let error = unsafe { bus_dmamap_load_mbuf(dmat, map, m, BUS_DMA_STREAMING | BUS_DMA_NOWAIT) };
    if error != Err(Errno::EFBIG) {
        return error;
    }

    m_defrag(m, M_DONTWAIT)?;

    // SAFETY: as above.
    unsafe { bus_dmamap_load_mbuf(dmat, map, m, BUS_DMA_STREAMING | BUS_DMA_NOWAIT) }
}

/// `vmxnet3_tx_offload`: the VLAN, checksum and TSO requests of `m` in its start-of-packet
/// descriptor `sop`.
pub fn vmxnet3_tx_offload(sop: &mut Vmxnet3Txdesc, m: &Mbuf) {
    let mut ext = EtherExtracted::new();
    let mut offset: u32;
    let ph = m.m_pkthdr();

    // VLAN Offload

    if NVLAN > 0 && m.m_flags().get() & M_VLANTAG != 0 {
        sop.tx_word3 |= VMXNET3_TX_VTAG_MODE.to_le();
        sop.tx_word3 |= ((u32::from(ph.ether_vtag.get()) & VMXNET3_TX_VLANTAG_M)
            << VMXNET3_TX_VLANTAG_S)
            .to_le();
    }

    // Checksum Offload

    if ph.csum_flags.get() & M_TCP_CSUM_OUT == 0 && ph.csum_flags.get() & M_UDP_CSUM_OUT == 0 {
        return;
    }

    ether_extract_headers(m, &mut ext);

    let mut hdrlen = size_of::<crate::netinet::if_ether::EtherHeader>() as u32;
    if !ext.evh.is_null() {
        hdrlen = size_of::<crate::netinet::if_ether::EtherVlanHeader>() as u32;
    }

    if !ext.ip4.is_null() || !ext.ip6.is_null() {
        hdrlen += ext.iphlen;
    }

    if !ext.tcp.is_null() {
        offset = hdrlen + TCPHDR_TH_SUM;
    } else if !ext.udp.is_null() {
        offset = hdrlen + UDPHDR_UH_SUM;
    } else {
        return;
    }

    if ph.csum_flags.get() & M_TCP_TSO == 0 {
        hdrlen &= VMXNET3_TX_HLEN_M;
        offset &= VMXNET3_TX_OP_M;

        sop.tx_word3 |= (VMXNET3_OM_CSUM << VMXNET3_TX_OM_S).to_le();
        sop.tx_word3 |= (hdrlen << VMXNET3_TX_HLEN_S).to_le();
        sop.tx_word2 |= (offset << VMXNET3_TX_OP_S).to_le();

        return;
    }

    // TCP Segmentation Offload

    if ext.tcp.is_null() || ph.ph_mss.get() == 0 {
        tcpstat_inc(TcpstatCounters::TcpsOutbadtso);
        return;
    }

    if !ext.ip4.is_null() {
        // SAFETY: ether_extract_headers found a whole IPv4 header at `ext.ip4` inside the
        // mbuf's data, possibly unaligned.
        unsafe { ptr::addr_of_mut!((*ext.ip4).ip_sum).write_unaligned(0) };
    }

    hdrlen += ext.tcphlen;
    hdrlen &= VMXNET3_TX_HLEN_M;

    let mss = u32::from(ph.ph_mss.get());
    sop.tx_word3 |= (VMXNET3_OM_TSO << VMXNET3_TX_OM_S).to_le();
    sop.tx_word3 |= (hdrlen << VMXNET3_TX_HLEN_S).to_le();
    sop.tx_word2 |= (mss << VMXNET3_TX_OP_S).to_le();

    let len = ph.len.get() as u32;
    tcpstat_add(
        TcpstatCounters::TcpsOutpkttso,
        u64::from(len.wrapping_sub(hdrlen).div_ceil(mss)),
    );
}

/// `vmxnet3_start`: transmit what the send queue holds.
pub fn vmxnet3_start(ifq: &'static Ifqueue) {
    let Some(ifp) = ifq.ifq_if.get() else {
        return;
    };
    let sc = vmxnet3_softc(ifp);
    let tq_ptr = ifq.ifq_softc.get().cast::<Vmxnet3Txqueue>().cast_const();
    if tq_ptr.is_null() {
        panic(format_args!("vmx: send queue without its tx queue"));
    }
    // SAFETY: vmxnet3_attach sets each send queue's `ifq_softc` to its queue's
    // `vmxnet3_txqueue`, which lives as long as the softc.
    let tq = unsafe { &*tq_ptr };
    let ring = &tq.cmd_ring;
    let dmat = sc.dmat();
    let mut post = false;

    let mut free = ring.cons.load(Ordering::Acquire);
    let mut prod = ring.prod.load(Ordering::Relaxed);
    if free <= prod {
        free += NTXDESC;
    }
    free -= prod;

    let rmap = ring.dmamem.map();
    bus_dmamap_sync(dmat, rmap, 0, ring.dmamem.len(), BUS_DMASYNC_POSTWRITE);

    let mut rgen = ring.r#gen.get();

    loop {
        if free <= NTXSEGS {
            break;
        }

        let Some(mut m) = ifq_dequeue(ifq) else {
            break;
        };

        // Headers for Ether, IP, TCP including options must lay in first mbuf to support
        // TSO.  Usually our stack gets that right. To avoid packet parsing here, make a rough
        // estimate for simple IPv4.  Cases seen in the wild contain only ether header in
        // separate mbuf.  To support IPv6 with TCP options, move as much as possible into
        // first mbuf.  Realloc mbuf before bus dma load.
        let mut hdrlen = (ETHER_HDR_LEN + size_of::<Ip>() + TCPHDR_LEN) as i32;
        if m.m_pkthdr().csum_flags.get() & M_TCP_TSO != 0
            && (m.m_len().get() as i32) < hdrlen
            && hdrlen <= m.m_pkthdr().len.get()
        {
            hdrlen = MHLEN as i32;
            // m_pullup preserves alignment, reserve space
            hdrlen -= (mtod::<u8>(m) as usize & (size_of::<u64>() - 1)) as i32;
            if hdrlen > m.m_pkthdr().len.get() {
                hdrlen = m.m_pkthdr().len.get();
            }
            match m_pullup(m, hdrlen) {
                Some(n) => m = n,
                None => {
                    ifq.ifq_errors.set(ifq.ifq_errors.get() + 1);
                    continue;
                }
            }
        }

        let map = ring.map(prod);

        if vmx_load_mbuf(dmat, map, m).is_err() {
            ifq.ifq_errors.set(ifq.ifq_errors.get() + 1);
            m_freem(m);
            continue;
        }

        // NBPFILTER > 0
        let if_bpf = ifp.if_bpf.get();
        if !if_bpf.is_null() {
            let _ = bpf_mtap_ether(if_bpf, m, BPF_DIRECTION_OUT);
        }

        let bpfp = tq.bpfp.get();
        let if_bpf = if bpfp.is_null() {
            ptr::null_mut()
        } else {
            // SAFETY: vmxnet3_attach points `bpfp` at the queue's `bpf` cookie, which lives
            // as long as the queue.
            unsafe { bpfp.read() }
        };
        if !if_bpf.is_null() {
            let _ = bpf_mtap_ether(if_bpf, m, BPF_DIRECTION_OUT);
        }

        ring.m[prod as usize].set(Some(m));

        bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_PREWRITE);

        let nsegs = map.dm_nsegs.get().max(0) as u32;
        free -= nsegs;
        // set oactive here since txintr may be triggered in parallel
        if free <= NTXSEGS {
            ifq_set_oactive(ifq);
        }

        let mut generation = rgen ^ VMX_TX_GEN;
        let sop = prod;
        let mut last = prod;
        for seg in &map.dm_segs()[..nsegs as usize] {
            let seg = seg.get();
            last = prod;
            ring.txd_set(
                prod,
                Vmxnet3Txdesc {
                    tx_addr: (seg.ds_addr as u64).to_le(),
                    tx_word2: ((seg.ds_len as u32) << VMXNET3_TX_LEN_S).to_le() | generation,
                    tx_word3: 0,
                },
            );

            prod += 1;
            if prod == NTXDESC {
                prod = 0;
                rgen ^= VMX_TX_GEN;
            }

            generation = rgen;
        }
        let mut txd = ring.txd_get(last);
        txd.tx_word3 = (VMXNET3_TX_EOP | VMXNET3_TX_COMPREQ).to_le();
        ring.txd_set(last, txd);

        let mut sopd = ring.txd_get(sop);
        vmxnet3_tx_offload(&mut sopd, m);
        ring.txd_set(sop, sopd);

        ring.prod.store(prod, Ordering::Release);
        // Change the ownership by flipping the "generation" bit
        bus_dmamap_sync(
            dmat,
            rmap,
            0,
            ring.dmamem.len(),
            BUS_DMASYNC_PREWRITE | BUS_DMASYNC_POSTWRITE,
        );
        let mut sopd = ring.txd_get(sop);
        sopd.tx_word2 ^= VMX_TX_GEN;
        ring.txd_set(sop, sopd);

        post = true;
    }

    bus_dmamap_sync(dmat, rmap, 0, ring.dmamem.len(), BUS_DMASYNC_PREWRITE);

    if !post {
        return;
    }

    ring.r#gen.set(rgen);

    sc.write_bar0(vmxnet3_bar0_txh(tq.queue.get()), prod);
}

/// `vmxnet3_watchdog`.
pub fn vmxnet3_watchdog(ifp: &'static Ifnet) {
    let sc = vmxnet3_softc(ifp);

    printf(format_args!(
        "{}: device timeout\n",
        Str(&ifp.if_xname.get())
    ));
    let s = splnet();
    let _ = vmxnet3_init(sc);
    splx(s);
}

/// `vmxnet3_media_status`.
pub fn vmxnet3_media_status(ifp: &'static Ifnet, ifmr: &mut Ifmediareq) {
    let sc = vmxnet3_softc(ifp);

    vmxnet3_link_state(sc);

    ifmr.ifm_status = IFM_AVALID;
    ifmr.ifm_active = IFM_ETHER;

    if ifp.if_link_state.get() != LINK_STATE_UP {
        return;
    }

    ifmr.ifm_status |= IFM_ACTIVE;

    if ifp.if_baudrate.get() >= if_gbps(10) {
        ifmr.ifm_active |= IFM_10G_T;
    }
}

/// `vmxnet3_media_change`.
pub fn vmxnet3_media_change(_ifp: &'static Ifnet) -> Result<(), Errno> {
    Ok(())
}

/// `vmxnet3_dma_allocmem`: `size` bytes of zeroed DMA memory aligned to `align`, mapped; the
/// mapping and its bus address. The map used to learn the address is destroyed again.
pub fn vmxnet3_dma_allocmem(
    sc: &Vmxnet3Softc,
    size: usize,
    align: BusSize,
) -> Result<(NonNull<u8>, BusAddr), Errno> {
    let t = sc.dmat();
    let mut segs = [BusDmaSegment::default(); 1];

    bus_dmamem_alloc(t, size, align, 0, &mut segs, BUS_DMA_NOWAIT | BUS_DMA_64BIT)?;
    let va = bus_dmamem_map(t, &mut segs, size, BUS_DMA_NOWAIT)?;
    let map = bus_dmamap_create(t, size, 1, size, 0, BUS_DMA_NOWAIT | BUS_DMA_64BIT)?;
    // SAFETY: `va` maps `size` bytes that are never freed; the map is unloaded below.
    unsafe { bus_dmamap_load(t, map, va.as_ptr(), size, None, BUS_DMA_NOWAIT) }?;
    // SAFETY: the `size` bytes just mapped, which nothing else uses yet.
    unsafe { ptr::write_bytes(va.as_ptr(), 0, size) };
    let pa = map.dm_segs()[0].get().ds_addr;
    bus_dmamap_unload(t, map);
    // SAFETY: the map made above, unloaded, which nothing else holds.
    unsafe { bus_dmamap_destroy(t, NonNull::from(map)) };
    Ok((va, pa))
}

/// `vmx_dmamem_alloc`: `size` bytes of zeroed DMA memory aligned to `align` in `vdm`,
/// mapped and loaded.
fn vmx_dmamem_alloc(
    sc: &Vmxnet3Softc,
    vdm: &VmxDmamem,
    size: BusSize,
    align: BusSize,
) -> Result<(), Errno> {
    let dmat = sc.dmat();
    vdm.vdm_size.set(size);

    let map = bus_dmamap_create(
        dmat,
        size,
        1,
        size,
        0,
        BUS_DMA_WAITOK | BUS_DMA_ALLOCNOW | BUS_DMA_64BIT,
    )
    .map_err(|_| Errno::ENOMEM)?;
    vdm.vdm_map.set(Some(map));
    let destroy = || {
        vdm.vdm_map.set(None);
        // SAFETY: the map made above, which nothing else holds.
        unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
    };

    let mut seg = [BusDmaSegment::default(); 1];
    let nsegs = match bus_dmamem_alloc(
        dmat,
        size,
        align,
        0,
        &mut seg,
        BUS_DMA_WAITOK | BUS_DMA_ZERO | BUS_DMA_64BIT,
    ) {
        Ok(n) => n,
        Err(_) => {
            // destroy:
            destroy();
            return Err(Errno::ENOMEM);
        }
    };
    vdm.vdm_seg.set(seg[0]);
    vdm.vdm_nsegs.set(nsegs as i32);

    let kva = match bus_dmamem_map(dmat, &mut seg[..nsegs], size, BUS_DMA_WAITOK) {
        Ok(kva) => kva,
        Err(_) => {
            // free:
            // SAFETY: the segment allocated above, not mapped.
            unsafe { bus_dmamem_free(dmat, &seg[..nsegs]) };
            destroy();
            return Err(Errno::ENOMEM);
        }
    };
    vdm.vdm_kva.set(kva.as_ptr());

    // SAFETY: `kva` maps `size` bytes that are never freed (vmx_dmamem_free is `notyet`).
    if unsafe { bus_dmamap_load(dmat, map, kva.as_ptr(), size, None, BUS_DMA_WAITOK) }.is_err() {
        // unmap:
        vdm.vdm_kva.set(ptr::null_mut());
        // SAFETY: the mapping made above, which nothing uses.
        unsafe { bus_dmamem_unmap(dmat, kva, size) };
        // free:
        // SAFETY: the segment allocated above, now unmapped.
        unsafe { bus_dmamem_free(dmat, &seg[..nsegs]) };
        // destroy:
        destroy();
        return Err(Errno::ENOMEM);
    }

    Ok(())
}

// notyet: vmx_dmamem_free; not compiled in C.

// NKSTAT > 0: vmx_kstat_rate, struct vmx_kstat_tpl, vmx_rx_kstat_tpl[], vmx_tx_kstat_tpl[],
// vmx_kstat_init, vmx_kstat_read, vmx_kstat_create, vmx_kstat_txstats and vmx_kstat_rxstats;
// kstat(4) is not configured.

const _: () = assert!(VMX_MAX_QUEUES == 8);
const _: () = assert!(VMX_MCAST_LEN == 4092);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests of vmx(4)'s pure parts: the device table, the generation bits, the zeroed
    // softc and queues, and the offload words `vmxnet3_tx_offload` and `vmxnet3_rx_offload`
    // derive from real packets; the reference-backed test reads the constants out of
    // `if_vmx.c` itself, where the C keeps its private ones.

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
            rxc_word3: VMXNET3_RXC_IPV4
                | VMXNET3_RXC_IPSUM_OK
                | VMXNET3_RXC_CSUM_OK
                | VMXNET3_RXC_TCP,
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
}
/* </TESTS> */
