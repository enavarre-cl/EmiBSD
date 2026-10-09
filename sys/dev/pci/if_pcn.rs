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
 * Copyright (c) 2001 Wasabi Systems, Inc.
 * All rights reserved.
 *
 * Written by Jason R. Thorpe for Wasabi Systems, Inc.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed for the NetBSD Project by
 *	Wasabi Systems, Inc.
 * 4. The name of Wasabi Systems, Inc. may not be used to endorse
 *    or promote products derived from this software without specific prior
 *    written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY WASABI SYSTEMS, INC. ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL WASABI SYSTEMS, INC
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! pcn(4): the AMD PCnet-PCI Ethernet driver (`dev/pci/if_pcn.c`).
//!
//! Upstream: sys/dev/pci/if_pcn.c @ 3ce1f3f79392
//!
//! Device driver for the AMD PCnet-PCI series (Am79c970, Am79c970A, Am79c971, Am79c972,
//! Am79c973/Am79c975) and the virtual PCnet-PCI of VMware and QEMU (`-nic model=pcnet`,
//! an Am79c970A). One control-data clump in `bus_dma(9)` memory holds the 512 transmit
//! descriptors, the 128 receive descriptors and the initialization block; every packet gets
//! its own DMA map, up to 16 segments for a transmission. The Am79c970 and Am79c970A have no
//! MII and use their own `ifmedia` (`pcn_79c970_mediainit`), the later chips attach mii(4)
//! PHYs.
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the device first and every member a `Cell`, as the other
//!   drivers' (`softc` is zero-initialised memory); `sc_variant`, the three description
//!   tables and the DMA map are `Option`s (all-zero valid). The descriptors live in the DMA
//!   clump, reached through a raw pointer by accessors that read and write one 32-bit field
//!   at a time, volatile, in the order the C stores them.
//! - The driver is not `MPSAFE`: `if_start` (not `if_qstart`), the interrupt at `IPL_NET`
//!   alone, so the kernel lock covers them as in C.
//! - `NBPFILTER > 0`: `bpf_mtap` is called when `if_bpf` is set.
//! - `PCN_NO_PROM` is not defined: the address is read from the address PROM.
//! - `pcn_set_filter`'s hash of the multicast addresses is [`pcn_hash_multi`], so it can be
//!   tested on the host.
//! - `pcn_start` and `pcn_init` keep the C's `lasttx = -1` and `error` as `Option` and
//!   `Result<(), Errno>`; `pcn_ioctl` returns the errno as `Result<(), Errno>`.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};

use crate::dev::ic::am79900reg::{
    LE_R1_BCNT_MASK, LE_R1_BUFF, LE_R1_CRC, LE_R1_ENP, LE_R1_ERR, LE_R1_FRAM, LE_R1_OFLO,
    LE_R1_ONES, LE_R1_OWN, LE_R1_STP, LE_T1_BCNT_MASK, LE_T1_ENP, LE_T1_ERR, LE_T1_LTINT,
    LE_T1_MORE, LE_T1_ONE, LE_T1_ONES, LE_T1_OWN, LE_T1_STP, LE_T2_BUFF, LE_T2_LCOL, LE_T2_RTRY,
    LE_T2_UFLO, Leinit, Lermd, Letmd,
};
use crate::dev::ic::lancereg::{
    LE_B2_ASEL, LE_B9_AUIFD, LE_B9_FDEN, LE_B18_BREADE, LE_B18_BWRITE, LE_B18_NOUFLO,
    LE_B20_SSTYLE_PCNETPCI2, LE_B20_SSTYLE_PCNETPCI3, LE_B32_DANAS, LE_BCR2, LE_BCR9, LE_BCR18,
    LE_BCR20, LE_BCR25, LE_BCR32, LE_BCR33, LE_BCR34, LE_C0_BABL, LE_C0_ERR, LE_C0_IDON,
    LE_C0_INEA, LE_C0_INIT, LE_C0_INTR, LE_C0_MERR, LE_C0_MISS, LE_C0_RINT, LE_C0_RXON, LE_C0_STOP,
    LE_C0_STRT, LE_C0_TDMD, LE_C0_TINT, LE_C0_TXON, LE_C3_DXSUFLO, LE_C3_IDONM, LE_C3_MISSM,
    LE_C4_APAD_XMT, LE_C4_DMAPLUS, LE_C4_MFCOM, LE_C4_RCVCCOM, LE_C4_TXSTRTM, LE_C5_LTINTEN,
    LE_C5_SINTE, LE_C5_SPND, LE_C7_FASTSPNDE, LE_C15_PROM, LE_C80_XMTSP_MAX, LE_CSR0, LE_CSR1,
    LE_CSR2, LE_CSR3, LE_CSR4, LE_CSR5, LE_CSR7, LE_CSR15, LE_CSR80, LE_CSR88, PARTID_Am79c970,
    PARTID_Am79c970A, PARTID_Am79c971, PARTID_Am79c972, PARTID_Am79c973, PARTID_Am79c975,
    PARTID_Am79c976, PARTID_Am79c978, PHYAD_SHIFT, PORTSEL_10T, PORTSEL_AUI, PORTSEL_MASK,
    PORTSEL_MII, chipid_partid, chipid_ver, le_bcnt, le_c15_portsel, le_c80_rcvfw, le_c80_xmtfw,
    le_c80_xmtsp,
};
use crate::dev::mii::mii::{mii_attach, mii_down, mii_mediachg, mii_pollstat, mii_tick};
use crate::dev::mii::miivar::{MII_OFFSET_ANY, MII_PHY_ANY};
use crate::dev::pci::pci::{pci_matchbyid, pci_set_powerstate};
use crate::dev::pci::pci_map::pci_mapreg_map;
use crate::dev::pci::pcidevs::{
    PCI_PRODUCT_AMD_PCHOME_PCI, PCI_PRODUCT_AMD_PCNET_PCI, PCI_PRODUCT_TRIDENT_4DWAVE_DX,
    PCI_VENDOR_AMD, PCI_VENDOR_TRIDENT,
};
use crate::dev::pci::pcireg::{
    PCI_CLASS_NETWORK, PCI_MAPREG_MEM_TYPE_32BIT, PCI_MAPREG_START, PCI_MAPREG_TYPE_IO,
    PCI_MAPREG_TYPE_MEM, PCI_PMCSR_STATE_D0, PciProductId, PciVendorId, pci_class, pci_product,
    pci_vendor,
};
use crate::dev::pci::pcivar::{PciAttachArgs, PciMatchid};
use crate::kassert;
use crate::kern::kern_timeout::{timeout_add_sec, timeout_del, timeout_set};
use crate::kern::subr_prf::{Str, panic, printf};
use crate::kern::uipc_mbuf::{m_clget, m_defrag, m_freem, m_gethdr, ml_enqueue};
use crate::machine::bus::{
    BUS_DMA_COHERENT, BUS_DMA_NOWAIT, BUS_DMA_READ, BUS_DMASYNC_POSTREAD, BUS_DMASYNC_POSTWRITE,
    BUS_DMASYNC_PREREAD, BUS_DMASYNC_PREWRITE, BusDmaSegment, BusDmaTag, BusDmamap, BusSize,
    BusSpaceHandle, BusSpaceTag, bus_dmamap_create, bus_dmamap_destroy, bus_dmamap_load,
    bus_dmamap_load_mbuf, bus_dmamap_sync, bus_dmamap_unload, bus_dmamem_alloc, bus_dmamem_free,
    bus_dmamem_map, bus_dmamem_unmap, bus_space_read_1, bus_space_read_2, bus_space_read_4,
    bus_space_write_4,
};
use crate::machine::cpu::delay;
use crate::machine::intr::{IPL_NET, splnet, splx};
use crate::machine::pci_machdep::{
    PciChipsetTag, pci_intr_establish, pci_intr_map, pci_intr_string,
};
use crate::net::bpf::{BPF_DIRECTION_OUT, bpf_mtap};
use crate::net::if_::{
    IFF_ALLMULTI, IFF_BROADCAST, IFF_MULTICAST, IFF_PROMISC, IFF_RUNNING, IFF_SIMPLEX, IFF_UP,
    IFNAMSIZ, Ifmediareq, if_attach, if_input,
};
use crate::net::if_ethersubr::{ether_crc32_le, ether_ifattach, ether_ioctl, ether_sprintf};
use crate::net::if_media::{
    IFM_10_5, IFM_10_T, IFM_AUTO, IFM_ETHER, IFM_FDX, IFM_IMASK, IFM_NONE, ifm_subtype,
    ifmedia_add, ifmedia_init, ifmedia_ioctl, ifmedia_set,
};
use crate::net::if_var::Ifnet;
use crate::net::ifq::{
    ifq_clr_oactive, ifq_dequeue, ifq_init_maxlen, ifq_is_oactive, ifq_restart, ifq_set_oactive,
};
use crate::netinet::if_ether::{
    ETHER_ADDR_LEN, ETHER_CRC_LEN, EtherMultistep, ether_first_multi, ether_next_multi,
};
use crate::sys::device::{CfMatch, Cfattach, Cfdriver, DV_IFNET, Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::mbuf::{M_DONTWAIT, MCLBYTES, MT_DATA, Mbuf, MbufList};
use crate::sys::param::PAGE_SIZE;
use crate::sys::sockio::{SIOCGIFMEDIA, SIOCSIFADDR, SIOCSIFFLAGS, SIOCSIFMEDIA};

/// `PCN_PCI_CBIO`: the I/O space register in PCI configuration space.
const PCN_PCI_CBIO: i32 = PCI_MAPREG_START;
/// `PCN_PCI_CBMEM`: the memory space register in PCI configuration space.
const PCN_PCI_CBMEM: i32 = PCI_MAPREG_START + 0x04;

/// `PCN32_APROM`: the address PROM, in DWord I/O mode.
const PCN32_APROM: BusSize = 0x00;
/// `PCN32_RDP`: the register data port.
const PCN32_RDP: BusSize = 0x10;
/// `PCN32_RAP`: the register address port.
const PCN32_RAP: BusSize = 0x14;
/// `PCN32_RESET`: reading it resets the chip.
const PCN32_RESET: BusSize = 0x18;
/// `PCN32_BDP`: the bus data port.
const PCN32_BDP: BusSize = 0x1c;

/// `PCN16_RESET`: the reset register, in Word I/O mode.
const PCN16_RESET: BusSize = 0x14;

/// `PCN_NTXSEGS`: DMA segments a packet may use. Transmit descriptor list size: allocate
/// enough descriptors for 128 pending transmissions, and 4 segments per packet. We give each
/// packet up to 16 segments but allocate the 512 descriptors only (not more than 512 Tx
/// descriptors exist), the transmit logic can deal with this.
const PCN_NTXSEGS: i32 = 16;

/// `PCN_TXQUEUELEN`: pending transmissions.
const PCN_TXQUEUELEN: usize = 128;
/// `PCN_TXQUEUELEN_MASK`.
const PCN_TXQUEUELEN_MASK: usize = PCN_TXQUEUELEN - 1;
/// `PCN_NTXDESC`: transmit descriptors.
const PCN_NTXDESC: usize = 512;
/// `PCN_NTXDESC_MASK`.
const PCN_NTXDESC_MASK: usize = PCN_NTXDESC - 1;

/// `PCN_NEXTTX(x)`.
const fn pcn_nexttx(x: usize) -> usize {
    (x + 1) & PCN_NTXDESC_MASK
}

/// `PCN_NEXTTXS(x)`.
const fn pcn_nexttxs(x: usize) -> usize {
    (x + 1) & PCN_TXQUEUELEN_MASK
}

/// `PCN_TXINTR_MASK`: Tx interrupt every N + 1 packets.
const PCN_TXINTR_MASK: usize = 7;

/// `PCN_NRXDESC`: receive descriptors. One Rx buffer per incoming packet.
const PCN_NRXDESC: usize = 128;
/// `PCN_NRXDESC_MASK`.
const PCN_NRXDESC_MASK: usize = PCN_NRXDESC - 1;

/// `PCN_NEXTRX(x)`.
const fn pcn_nextrx(x: usize) -> usize {
    (x + 1) & PCN_NRXDESC_MASK
}

/// `struct pcn_control_data`: control structures are DMA'd to the PCnet chip. They are
/// allocated in a single clump that maps to a single DMA segment.
#[repr(C)]
pub struct PcnControlData {
    /// `pcd_txdescs`: the transmit descriptors.
    pub pcd_txdescs: [Letmd; PCN_NTXDESC],
    /// `pcd_rxdescs`: the receive descriptors.
    pub pcd_rxdescs: [Lermd; PCN_NRXDESC],
    /// `pcd_initblock`: the init block.
    pub pcd_initblock: Leinit,
}

/// `PCN_CDTXOFF(x)`: where transmit descriptor `x` is in the clump.
const fn pcn_cdtxoff(x: usize) -> usize {
    offset_of!(PcnControlData, pcd_txdescs) + x * size_of::<Letmd>()
}

/// `PCN_CDRXOFF(x)`: where receive descriptor `x` is in the clump.
const fn pcn_cdrxoff(x: usize) -> usize {
    offset_of!(PcnControlData, pcd_rxdescs) + x * size_of::<Lermd>()
}

/// `PCN_CDINITOFF`: where the init block is in the clump.
const PCN_CDINITOFF: usize = offset_of!(PcnControlData, pcd_initblock);

/// `struct pcn_txsoft`: software state for transmit jobs.
#[repr(C)]
pub struct PcnTxsoft {
    /// `txs_mbuf`: head of our mbuf chain.
    pub txs_mbuf: Cell<Option<&'static Mbuf>>,
    /// `txs_dmamap`: our DMA map.
    pub txs_dmamap: Cell<Option<&'static BusDmamap>>,
    /// `txs_firstdesc`: first descriptor in packet.
    pub txs_firstdesc: Cell<usize>,
    /// `txs_lastdesc`: last descriptor in packet.
    pub txs_lastdesc: Cell<usize>,
}

/// `struct pcn_rxsoft`: software state for receive jobs.
#[repr(C)]
pub struct PcnRxsoft {
    /// `rxs_mbuf`: head of our mbuf chain.
    pub rxs_mbuf: Cell<Option<&'static Mbuf>>,
    /// `rxs_dmamap`: our DMA map.
    pub rxs_dmamap: Cell<Option<&'static BusDmamap>>,
}

/// `pcn_79c970_rcvfw[]`: Rx FIFO watermarks of the Am79c970 (without the NULL).
static PCN_79C970_RCVFW: [&str; 3] = ["16 bytes", "64 bytes", "128 bytes"];

/// `pcn_79c971_rcvfw[]`.
static PCN_79C971_RCVFW: [&str; 3] = ["16 bytes", "64 bytes", "112 bytes"];

/// `pcn_79c970_xmtsp[]`: Tx start points of the Am79c970.
static PCN_79C970_XMTSP: [&str; 4] = ["8 bytes", "64 bytes", "128 bytes", "248 bytes"];

/// `pcn_79c971_xmtsp[]`.
static PCN_79C971_XMTSP: [&str; 4] = ["20 bytes", "64 bytes", "128 bytes", "248 bytes"];

/// `pcn_79c971_xmtsp_sram[]`.
static PCN_79C971_XMTSP_SRAM: [&str; 4] =
    ["44 bytes", "64 bytes", "128 bytes", "store-and-forward"];

/// `pcn_79c970_xmtfw[]`: Tx FIFO watermarks of the Am79c970.
static PCN_79C970_XMTFW: [&str; 3] = ["16 bytes", "64 bytes", "128 bytes"];

/// `pcn_79c971_xmtfw[]`.
static PCN_79C971_XMTFW: [&str; 3] = ["16 bytes", "64 bytes", "108 bytes"];

/// `PCN_F_HAS_MII`: `sc_flags`: has MII.
const PCN_F_HAS_MII: i32 = 0x0001;

/// `struct pcn_softc`: software state per device.
#[repr(C)]
pub struct PcnSoftc {
    /// `sc_dev`: generic device information.
    pub sc_dev: Device,
    /// `sc_st`: bus space tag.
    pub sc_st: Cell<Option<BusSpaceTag>>,
    /// `sc_sh`: bus space handle.
    pub sc_sh: Cell<Option<BusSpaceHandle>>,
    /// `sc_dmat`: bus DMA tag.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_arpcom`: Ethernet common data.
    pub sc_arpcom: crate::netinet::if_ether::Arpcom,

    /// `sc_variant`: points to our media routines, etc.
    pub sc_variant: Cell<Option<&'static PcnVariant>>,

    /// `sc_ih`: interrupt cookie.
    pub sc_ih: Cell<*mut c_void>,

    /// `sc_mii`: MII/media information.
    pub sc_mii: crate::dev::mii::miivar::MiiData,

    /// `sc_tick_timeout`: tick timeout.
    pub sc_tick_timeout: crate::sys::timeout::Timeout,

    /// `sc_cddmamap`: control data DMA map (`sc_cddma` is its first segment's address).
    pub sc_cddmamap: Cell<Option<&'static BusDmamap>>,

    /// `sc_txsoft`: software state for transmit descriptors.
    pub sc_txsoft: [PcnTxsoft; PCN_TXQUEUELEN],
    /// `sc_rxsoft`: software state for receive descriptors.
    pub sc_rxsoft: [PcnRxsoft; PCN_NRXDESC],

    /// `sc_control_data`: control data structures.
    pub sc_control_data: Cell<*mut PcnControlData>,

    /// `sc_rcvfw_desc`: Rx FIFO watermark info.
    pub sc_rcvfw_desc: Cell<Option<&'static [&'static str]>>,
    /// `sc_rcvfw`.
    pub sc_rcvfw: Cell<u32>,

    /// `sc_xmtsp_desc`: Tx start point info.
    pub sc_xmtsp_desc: Cell<Option<&'static [&'static str]>>,
    /// `sc_xmtsp`.
    pub sc_xmtsp: Cell<u32>,

    /// `sc_xmtfw_desc`: Tx FIFO watermark info.
    pub sc_xmtfw_desc: Cell<Option<&'static [&'static str]>>,
    /// `sc_xmtfw`.
    pub sc_xmtfw: Cell<u32>,

    /// `sc_flags`: misc. flags (`PCN_F_*`).
    pub sc_flags: Cell<i32>,
    /// `sc_swstyle`: the software style in use.
    pub sc_swstyle: Cell<u32>,

    /// `sc_txfree`: number of free Tx descriptors.
    pub sc_txfree: Cell<i32>,
    /// `sc_txnext`: next ready Tx descriptor.
    pub sc_txnext: Cell<usize>,

    /// `sc_txsfree`: number of free Tx jobs.
    pub sc_txsfree: Cell<i32>,
    /// `sc_txsnext`: next free Tx job.
    pub sc_txsnext: Cell<usize>,
    /// `sc_txsdirty`: dirty Tx jobs.
    pub sc_txsdirty: Cell<usize>,

    /// `sc_rxptr`: next ready Rx descriptor/job.
    pub sc_rxptr: Cell<usize>,

    /// `sc_csr5`: prototype CSR5 register.
    pub sc_csr5: Cell<u32>,
    /// `sc_mode`: prototype MODE register.
    pub sc_mode: Cell<u32>,
}

// SAFETY: `#[repr(C)]` with the `Device` first; every other member is a `Cell` of an
// all-zero-valid type (integers, `Option`s of references, raw pointers), a timeout or the
// zero-valid arpcom/mii structures, which the other drivers' softcs hold the same way.
unsafe impl Softc for PcnSoftc {}

/// The `desc_accessors!` below: volatile reads and writes of one 32-bit word of a descriptor
/// in the DMA clump.
macro_rules! desc_accessors {
    ($get:ident, $set:ident, $ptr:ident, $field:ident) => {
        #[doc = concat!("Reads `", stringify!($field), "` of descriptor `x`.")]
        fn $get(&self, x: usize) -> u32 {
            let p = self.$ptr(x);
            // SAFETY: `$ptr` returns the address of descriptor `x` of the control data mapped
            // by `pcn_attach` (the index is checked there); no reference is made.
            unsafe { ptr::read_volatile(ptr::addr_of!((*p).$field)) }
        }

        #[doc = concat!("Writes `", stringify!($field), "` of descriptor `x`.")]
        fn $set(&self, x: usize, v: u32) {
            let p = self.$ptr(x);
            // SAFETY: as for the reader; the chip reads the word only after the descriptor
            // is handed over (OWN), the driver's stores are ordered as the C's.
            unsafe { ptr::write_volatile(ptr::addr_of_mut!((*p).$field), v) }
        }
    };
}

impl PcnSoftc {
    /// `sc->sc_dev.dv_xname`.
    fn devname(&self) -> &str {
        self.sc_dev.xname()
    }

    /// `sc->sc_dmat`, which `pcn_attach` sets first.
    fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => panic(format_args!("{}: no DMA tag", self.devname())),
        }
    }

    /// `sc->sc_st` and `sc->sc_sh`, which `pcn_attach` maps first.
    fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.sc_st.get(), self.sc_sh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("{}: registers not mapped", self.devname())),
        }
    }

    /// `&sc->sc_arpcom.ac_if`.
    fn ifp(&'static self) -> &'static Ifnet {
        &self.sc_arpcom.ac_if
    }

    /// `sc->sc_variant`, which `pcn_attach` sets.
    fn variant(&self) -> &'static PcnVariant {
        match self.sc_variant.get() {
            Some(v) => v,
            None => panic(format_args!("{}: no variant", self.devname())),
        }
    }

    /// `sc->sc_cddmamap`.
    fn cddmamap(&self) -> &'static BusDmamap {
        match self.sc_cddmamap.get() {
            Some(m) => m,
            None => panic(format_args!("{}: no control data map", self.devname())),
        }
    }

    /// `sc_cddma`: `sc_cddmamap->dm_segs[0].ds_addr`.
    fn cddma(&self) -> usize {
        self.cddmamap().dm_segs()[0].get().ds_addr
    }

    /// `PCN_CDTXADDR(sc, x)`.
    fn cdtxaddr(&self, x: usize) -> usize {
        self.cddma() + pcn_cdtxoff(x)
    }

    /// `PCN_CDRXADDR(sc, x)`.
    fn cdrxaddr(&self, x: usize) -> usize {
        self.cddma() + pcn_cdrxoff(x)
    }

    /// `PCN_CDINITADDR(sc)`.
    fn cdinitaddr(&self) -> usize {
        self.cddma() + PCN_CDINITOFF
    }

    /// `PCN_CDTXSYNC(sc, x, n, ops)`.
    fn cdtxsync(&self, x: usize, n: usize, ops: i32) {
        let mut x = x;
        let mut n = n;

        // If it will wrap around, sync to the end of the ring.
        if x + n > PCN_NTXDESC {
            bus_dmamap_sync(
                self.dmat(),
                self.cddmamap(),
                pcn_cdtxoff(x),
                size_of::<Letmd>() * (PCN_NTXDESC - x),
                ops,
            );
            n -= PCN_NTXDESC - x;
            x = 0;
        }

        // Now sync whatever is left.
        bus_dmamap_sync(
            self.dmat(),
            self.cddmamap(),
            pcn_cdtxoff(x),
            size_of::<Letmd>() * n,
            ops,
        );
    }

    /// `PCN_CDRXSYNC(sc, x, ops)`.
    fn cdrxsync(&self, x: usize, ops: i32) {
        bus_dmamap_sync(
            self.dmat(),
            self.cddmamap(),
            pcn_cdrxoff(x),
            size_of::<Lermd>(),
            ops,
        );
    }

    /// `PCN_CDINITSYNC(sc, ops)`.
    fn cdinitsync(&self, ops: i32) {
        bus_dmamap_sync(
            self.dmat(),
            self.cddmamap(),
            PCN_CDINITOFF,
            size_of::<Leinit>(),
            ops,
        );
    }

    /// `&sc->sc_txdescs[x]`.
    fn txd_ptr(&self, x: usize) -> *mut Letmd {
        kassert!(x < PCN_NTXDESC);
        let base = self.sc_control_data.get();
        kassert!(!base.is_null());
        // SAFETY: `base` is the control data `pcn_attach` mapped, `PcnControlData` sized, and
        // `x` is in bounds; the address is computed without making a reference.
        unsafe { ptr::addr_of_mut!((*base).pcd_txdescs[x]) }
    }

    /// `&sc->sc_rxdescs[x]`.
    fn rxd_ptr(&self, x: usize) -> *mut Lermd {
        kassert!(x < PCN_NRXDESC);
        let base = self.sc_control_data.get();
        kassert!(!base.is_null());
        // SAFETY: as for `txd_ptr`.
        unsafe { ptr::addr_of_mut!((*base).pcd_rxdescs[x]) }
    }

    /// `&sc->sc_initblock`.
    fn init_ptr(&self) -> *mut Leinit {
        let base = self.sc_control_data.get();
        kassert!(!base.is_null());
        // SAFETY: as for `txd_ptr`.
        unsafe { ptr::addr_of_mut!((*base).pcd_initblock) }
    }

    desc_accessors!(tmd0, set_tmd0, txd_ptr, tmd0);
    desc_accessors!(tmd1, set_tmd1, txd_ptr, tmd1);
    desc_accessors!(tmd2, set_tmd2, txd_ptr, tmd2);
    desc_accessors!(rmd0, set_rmd0, rxd_ptr, rmd0);
    desc_accessors!(rmd1, set_rmd1, rxd_ptr, rmd1);
    desc_accessors!(rmd2, set_rmd2, rxd_ptr, rmd2);

    /// `memset(sc->sc_txdescs, 0, sizeof(sc->sc_txdescs))`.
    fn clear_txdescs(&self) {
        let p = self.txd_ptr(0);
        // SAFETY: the whole transmit ring is `PCN_NTXDESC` descriptors from `txd_ptr(0)` in
        // the control data; the chip is stopped (`pcn_stop`, `pcn_reset`) when this runs.
        unsafe { ptr::write_bytes(p, 0, PCN_NTXDESC) }
    }

    /// Reads the init block.
    fn initblock(&self) -> Leinit {
        // SAFETY: `init_ptr` is the init block in the mapped control data.
        unsafe { ptr::read_volatile(self.init_ptr()) }
    }

    /// Writes the init block.
    fn set_initblock(&self, v: Leinit) {
        // SAFETY: as for `initblock`; the chip reads the block only after `LE_C0_INIT`.
        unsafe { ptr::write_volatile(self.init_ptr(), v) }
    }
}

/// `(struct pcn_softc *)ifp->if_softc`.
fn pcn_softc(ifp: &Ifnet) -> &'static PcnSoftc {
    let p = ifp.if_softc.get().cast::<PcnSoftc>().cast_const();
    if p.is_null() {
        panic(format_args!("pcn: interface without its softc"));
    }
    // SAFETY: pcn_attach sets `if_softc` to its softc and installs pcn's functions only on
    // its own interface; softcs outlive their interfaces.
    unsafe { &*p }
}

/// The softc behind the C's `void *` argument (interrupt, timeout).
fn pcn_arg(arg: *mut c_void) -> &'static PcnSoftc {
    if arg.is_null() {
        panic(format_args!("pcn: no softc argument"));
    }
    // SAFETY: pcn_attach passes the softc, which is never freed while the device exists.
    unsafe { &*arg.cast::<PcnSoftc>() }
}

/// The softc of the device `self_` (`(struct pcn_softc *)self`).
fn pcn_dev_sc(dev: &Device) -> &'static PcnSoftc {
    // SAFETY: the device was made for `pcn_ca`, whose softc is a `PcnSoftc`; softcs are never
    // freed while the device exists.
    unsafe { &*ptr::from_ref(dev.softc::<PcnSoftc>()) }
}

/// `struct pcn_variant`: description of a PCnet-PCI variant. Used to select media access
/// method, mostly, and to print a nice description of the chip.
pub struct PcnVariant {
    /// `pcv_desc`.
    pub pcv_desc: &'static str,
    /// `pcv_mediainit`.
    pub pcv_mediainit: fn(&'static PcnSoftc),
    /// `pcv_chipid`.
    pub pcv_chipid: u32,
}

/// `pcn_variants[]`, with the `"Unknown"` entry (chip id 0) last.
pub static PCN_VARIANTS: [PcnVariant; 9] = [
    PcnVariant {
        pcv_desc: "Am79c970",
        pcv_mediainit: pcn_79c970_mediainit,
        pcv_chipid: PARTID_Am79c970,
    },
    PcnVariant {
        pcv_desc: "Am79c970A",
        pcv_mediainit: pcn_79c970_mediainit,
        pcv_chipid: PARTID_Am79c970A,
    },
    PcnVariant {
        pcv_desc: "Am79c971",
        pcv_mediainit: pcn_79c971_mediainit,
        pcv_chipid: PARTID_Am79c971,
    },
    PcnVariant {
        pcv_desc: "Am79c972",
        pcv_mediainit: pcn_79c971_mediainit,
        pcv_chipid: PARTID_Am79c972,
    },
    PcnVariant {
        pcv_desc: "Am79c973",
        pcv_mediainit: pcn_79c971_mediainit,
        pcv_chipid: PARTID_Am79c973,
    },
    PcnVariant {
        pcv_desc: "Am79c975",
        pcv_mediainit: pcn_79c971_mediainit,
        pcv_chipid: PARTID_Am79c975,
    },
    PcnVariant {
        pcv_desc: "Am79c976",
        pcv_mediainit: pcn_79c971_mediainit,
        pcv_chipid: PARTID_Am79c976,
    },
    PcnVariant {
        pcv_desc: "Am79c978",
        pcv_mediainit: pcn_79c971_mediainit,
        pcv_chipid: PARTID_Am79c978,
    },
    PcnVariant {
        pcv_desc: "Unknown",
        pcv_mediainit: pcn_79c971_mediainit,
        pcv_chipid: 0,
    },
];

/// `pcn_copy_small`: copy small packets into a header mbuf (off).
static PCN_COPY_SMALL: i32 = 0;

/// `pcn_ca`.
pub static PCN_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<PcnSoftc>(),
    ca_match: Some(pcn_match),
    ca_attach: pcn_attach,
    ca_detach: None,
    ca_activate: None,
};

/// `pcn_devices[]`.
static PCN_DEVICES: [PciMatchid; 2] = [
    PciMatchid {
        pm_vid: PCI_VENDOR_AMD as PciVendorId,
        pm_pid: PCI_PRODUCT_AMD_PCNET_PCI as PciProductId,
    },
    PciMatchid {
        pm_vid: PCI_VENDOR_AMD as PciVendorId,
        pm_pid: PCI_PRODUCT_AMD_PCHOME_PCI as PciProductId,
    },
];

/// `pcn_cd`.
pub static PCN_CD: Cfdriver = Cfdriver::new(b"pcn", DV_IFNET, 0);

/// `pcn_csr_read`: routines to read and write the PCnet-PCI CSR/BCR space.
fn pcn_csr_read(sc: &PcnSoftc, reg: u32) -> u32 {
    let (t, h) = sc.regs();
    bus_space_write_4(t, h, PCN32_RAP, reg);
    bus_space_read_4(t, h, PCN32_RDP)
}

/// `pcn_csr_write`.
fn pcn_csr_write(sc: &PcnSoftc, reg: u32, val: u32) {
    let (t, h) = sc.regs();
    bus_space_write_4(t, h, PCN32_RAP, reg);
    bus_space_write_4(t, h, PCN32_RDP, val);
}

/// `pcn_bcr_read`.
fn pcn_bcr_read(sc: &PcnSoftc, reg: u32) -> u32 {
    let (t, h) = sc.regs();
    bus_space_write_4(t, h, PCN32_RAP, reg);
    bus_space_read_4(t, h, PCN32_BDP)
}

/// `pcn_bcr_write`.
fn pcn_bcr_write(sc: &PcnSoftc, reg: u32, val: u32) {
    let (t, h) = sc.regs();
    bus_space_write_4(t, h, PCN32_RAP, reg);
    bus_space_write_4(t, h, PCN32_BDP, val);
}

/// `pcn_lookup_variant`: unknown chips are treated like a generic PCnet-FAST (the last
/// entry).
pub fn pcn_lookup_variant(chipid: u32) -> &'static PcnVariant {
    let (known, unknown) = PCN_VARIANTS.split_at(PCN_VARIANTS.len() - 1);
    known
        .iter()
        .find(|pcv| pcv.pcv_chipid == chipid)
        .unwrap_or(&unknown[0])
}

/// `pcn_match`.
pub fn pcn_match(_parent: Option<&Device>, _match: &CfMatch, aux: *mut c_void) -> i32 {
    // SAFETY: pci attaches its children with a `pci_attach_args`.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };

    // IBM makes a PCI variant of this card which shows up as a Trident Microsystems 4DWAVE DX
    // (ethernet network, revision 0x25) this card is truly a pcn card, so we have a special
    // case match for it.
    if pci_vendor(pa.pa_id) == PCI_VENDOR_TRIDENT
        && pci_product(pa.pa_id) == PCI_PRODUCT_TRIDENT_4DWAVE_DX
        && pci_class(pa.pa_class) == PCI_CLASS_NETWORK
    {
        return 1;
    }

    pci_matchbyid(pa, &PCN_DEVICES)
}

/// What `pcn_attach` has made when it fails, for the unwinding of the C's `fail_*` labels.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum PcnAttachStage {
    /// Nothing yet (`fail_1` frees the memory).
    Alloc,
    /// The memory is mapped (`fail_2`).
    Map,
    /// The control data map exists (`fail_3`).
    Create,
    /// The control data map is loaded (`fail_4`).
    Load,
    /// The transmit maps exist (`fail_5`).
    TxMaps,
}

/// The C's `fail_*` labels: undoes what `pcn_attach` made up to `stage`, in reverse.
fn pcn_attach_unwind(sc: &PcnSoftc, seg: &[BusDmaSegment], stage: PcnAttachStage) {
    let dmat = sc.dmat();

    if stage >= PcnAttachStage::TxMaps {
        // fail_5
        for rxs in sc.sc_rxsoft.iter() {
            if let Some(map) = rxs.rxs_dmamap.take() {
                // SAFETY: a map made by `pcn_attach` for this slot, loaded with nothing.
                unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
            }
        }
    }
    if stage >= PcnAttachStage::Load {
        // fail_4
        for txs in sc.sc_txsoft.iter() {
            if let Some(map) = txs.txs_dmamap.take() {
                // SAFETY: a map made by `pcn_attach` for this slot, loaded with nothing.
                unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
            }
        }
        bus_dmamap_unload(dmat, sc.cddmamap());
    }
    if stage >= PcnAttachStage::Create {
        // fail_3
        if let Some(map) = sc.sc_cddmamap.take() {
            // SAFETY: the control data map made by `pcn_attach`, unloaded above.
            unsafe { bus_dmamap_destroy(dmat, NonNull::from(map)) };
        }
    }
    if stage >= PcnAttachStage::Map {
        // fail_2
        if let Some(kva) = NonNull::new(sc.sc_control_data.replace(ptr::null_mut()).cast::<u8>()) {
            // SAFETY: the mapping `pcn_attach` made of the control data; nothing uses it.
            unsafe { bus_dmamem_unmap(dmat, kva, size_of::<PcnControlData>()) };
        }
    }
    // fail_1
    // SAFETY: the segment `pcn_attach` allocated, no longer mapped.
    unsafe { bus_dmamem_free(dmat, seg) };
}

/// `pcn_attach`.
pub fn pcn_attach(_parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let sc = pcn_dev_sc(self_);
    // SAFETY: pci attaches its children with a `pci_attach_args` it owns for the duration of
    // the attach.
    let pa = unsafe { &*aux.cast::<PciAttachArgs>() };
    let pc: PciChipsetTag = pa.pa_pc;
    let arg: *mut c_void = ptr::from_ref(sc).cast_mut().cast();

    timeout_set(&sc.sc_tick_timeout, pcn_tick, arg);

    // Map the device.
    let io = pci_mapreg_map(pa, PCN_PCI_CBIO, PCI_MAPREG_TYPE_IO, 0, 0);
    let mem = pci_mapreg_map(
        pa,
        PCN_PCI_CBMEM,
        PCI_MAPREG_TYPE_MEM | PCI_MAPREG_MEM_TYPE_32BIT,
        0,
        0,
    );

    if let Ok((t, h, _, _)) = mem {
        sc.sc_st.set(Some(t));
        sc.sc_sh.set(Some(h));
    } else if let Ok((t, h, _, _)) = io {
        sc.sc_st.set(Some(t));
        sc.sc_sh.set(Some(h));
    } else {
        printf(format_args!(": unable to map device registers\n"));
        return;
    }

    sc.sc_dmat.set(Some(pa.pa_dmat));

    // Get it out of power save mode, if needed.
    pci_set_powerstate(pc, pa.pa_tag, PCI_PMCSR_STATE_D0 as i32);

    // Reset the chip to a known state. This also puts the chip into 32-bit mode.
    pcn_reset(sc);

    // Read the Ethernet address from the EEPROM (PCN_NO_PROM is not defined).
    let (st, sh) = sc.regs();
    let mut enaddr = [0u8; ETHER_ADDR_LEN];
    for (i, b) in enaddr.iter_mut().enumerate() {
        *b = bus_space_read_1(st, sh, PCN32_APROM + i);
    }

    // Now that the device is mapped, attempt to figure out what kind of chip we have. Note
    // that IDL has all 32 bits of the chip ID when we're in 32-bit mode.
    let chipid = pcn_csr_read(sc, LE_CSR88);
    sc.sc_variant
        .set(Some(pcn_lookup_variant(chipid_partid(chipid))));

    // Map and establish our interrupt.
    let Some(ih) = pci_intr_map(pa) else {
        printf(format_args!(": unable to map interrupt\n"));
        return;
    };
    let intrstr = pci_intr_string(pc, ih);
    let Some(cookie) = pci_intr_establish(pc, ih, IPL_NET, pcn_intr, arg, sc.devname()) else {
        printf(format_args!(
            ": unable to establish interrupt at {}\n",
            Str(intrstr.as_bytes())
        ));
        return;
    };
    sc.sc_ih.set(cookie.as_ptr());

    // Allocate the control data structures, and create and load the DMA map for it.
    let dmat = sc.dmat();
    let mut seg = [BusDmaSegment::default(); 1];
    let rseg = match bus_dmamem_alloc(dmat, size_of::<PcnControlData>(), PAGE_SIZE, 0, &mut seg, 0)
    {
        Ok(n) => n,
        Err(error) => {
            printf(format_args!(
                ": unable to allocate control data, error = {}\n",
                error as i32
            ));
            return;
        }
    };
    let seg = &mut seg[..rseg];

    match bus_dmamem_map(dmat, seg, size_of::<PcnControlData>(), BUS_DMA_COHERENT) {
        Ok(kva) => sc.sc_control_data.set(kva.as_ptr().cast()),
        Err(error) => {
            printf(format_args!(
                ": unable to map control data, error = {}\n",
                error as i32
            ));
            pcn_attach_unwind(sc, seg, PcnAttachStage::Alloc);
            return;
        }
    }

    match bus_dmamap_create(
        dmat,
        size_of::<PcnControlData>(),
        1,
        size_of::<PcnControlData>(),
        0,
        0,
    ) {
        Ok(map) => sc.sc_cddmamap.set(Some(map)),
        Err(error) => {
            printf(format_args!(
                ": unable to create control data DMA map, error = {}\n",
                error as i32
            ));
            pcn_attach_unwind(sc, seg, PcnAttachStage::Map);
            return;
        }
    }

    // SAFETY: the control data stays mapped until `pcn_attach_unwind` unloads the map and
    // unmaps it; the chip is not pointed at it before `pcn_init`.
    if let Err(error) = unsafe {
        bus_dmamap_load(
            dmat,
            sc.cddmamap(),
            sc.sc_control_data.get().cast(),
            size_of::<PcnControlData>(),
            None,
            0,
        )
    } {
        printf(format_args!(
            ": unable to load control data DMA map, error = {}\n",
            error as i32
        ));
        pcn_attach_unwind(sc, seg, PcnAttachStage::Create);
        return;
    }

    // Create the transmit buffer DMA maps.
    for (i, txs) in sc.sc_txsoft.iter().enumerate() {
        match bus_dmamap_create(dmat, MCLBYTES, PCN_NTXSEGS, MCLBYTES, 0, 0) {
            Ok(map) => txs.txs_dmamap.set(Some(map)),
            Err(error) => {
                printf(format_args!(
                    ": unable to create tx DMA map {}, error = {}\n",
                    i, error as i32
                ));
                pcn_attach_unwind(sc, seg, PcnAttachStage::Load);
                return;
            }
        }
    }

    // Create the receive buffer DMA maps.
    for (i, rxs) in sc.sc_rxsoft.iter().enumerate() {
        match bus_dmamap_create(dmat, MCLBYTES, 1, MCLBYTES, 0, 0) {
            Ok(map) => rxs.rxs_dmamap.set(Some(map)),
            Err(error) => {
                printf(format_args!(
                    ": unable to create rx DMA map {}, error = {}\n",
                    i, error as i32
                ));
                pcn_attach_unwind(sc, seg, PcnAttachStage::TxMaps);
                return;
            }
        }
        rxs.rxs_mbuf.set(None);
    }

    printf(format_args!(
        ", {}, rev {}: {}, address {}\n",
        sc.variant().pcv_desc,
        chipid_ver(chipid),
        Str(intrstr.as_bytes()),
        Str(&ether_sprintf(&enaddr))
    ));

    // Initialize our media structures.
    (sc.variant().pcv_mediainit)(sc);

    // Initialize FIFO watermark info.
    let chipid = sc.variant().pcv_chipid;
    // (The chip ids are `PARTID_*` constants, not upper-case names `match` accepts.)
    if chipid == PARTID_Am79c970 || chipid == PARTID_Am79c970A {
        sc.sc_rcvfw_desc.set(Some(&PCN_79C970_RCVFW));
        sc.sc_xmtsp_desc.set(Some(&PCN_79C970_XMTSP));
        sc.sc_xmtfw_desc.set(Some(&PCN_79C970_XMTFW));
    } else {
        {
            sc.sc_rcvfw_desc.set(Some(&PCN_79C971_RCVFW));
            // Read BCR25 to determine how much SRAM is on the board. If > 0, then we the chip
            // uses different Start Point thresholds.
            //
            // Note BCR25 and BCR26 are loaded from the EEPROM on RST, and unaffected by
            // S_RESET, so we don't really have to worry about them except for this.
            let reg = pcn_bcr_read(sc, LE_BCR25) & 0x00ff;
            if reg != 0 {
                sc.sc_xmtsp_desc.set(Some(&PCN_79C971_XMTSP_SRAM));
            } else {
                sc.sc_xmtsp_desc.set(Some(&PCN_79C971_XMTSP));
            }
            sc.sc_xmtfw_desc.set(Some(&PCN_79C971_XMTFW));
        }
    }

    // Set up defaults -- see the tables above for what these values mean.
    //
    // XXX How should we tune RCVFW and XMTFW?
    sc.sc_rcvfw.set(1); // minimum for full-duplex
    sc.sc_xmtsp.set(1);
    sc.sc_xmtfw.set(0);

    let ifp = sc.ifp();
    sc.sc_arpcom.ac_enaddr.set(enaddr);
    let mut xname = [0u8; IFNAMSIZ];
    let name = sc.sc_dev.dv_xname.get();
    let n = name
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(name.len())
        .min(IFNAMSIZ - 1);
    xname[..n].copy_from_slice(&name[..n]);
    ifp.if_xname.set(xname);
    ifp.if_softc.set(arg);
    ifp.if_flags
        .set(IFF_BROADCAST | IFF_SIMPLEX | IFF_MULTICAST);
    ifp.if_ioctl.set(Some(pcn_ioctl));
    ifp.if_start.set(Some(pcn_start));
    ifp.if_watchdog.set(Some(pcn_watchdog));
    ifq_init_maxlen(&ifp.if_snd, (PCN_NTXDESC - 1) as u32);

    // Attach the interface.
    if_attach(ifp);
    ether_ifattach(&sc.sc_arpcom);
}

/// `pcn_start`: [ifnet interface function] start packet transmission on the interface.
pub fn pcn_start(ifp: &'static Ifnet) {
    let sc = pcn_softc(ifp);

    if ifp.if_flags.get() & IFF_RUNNING == 0 || ifq_is_oactive(&ifp.if_snd) {
        return;
    }

    // Remember the previous number of free descriptors and the first descriptor we'll use.
    let ofree = sc.sc_txfree.get();
    let dmat = sc.dmat();

    // Loop through the send queue, setting up transmit descriptors until we drain the queue,
    // or use up all available transmit descriptors.
    loop {
        if sc.sc_txsfree.get() == 0 || sc.sc_txfree.get() < PCN_NTXSEGS + 1 {
            ifq_set_oactive(&ifp.if_snd);
            break;
        }

        // Grab a packet off the queue.
        let Some(m0) = ifq_dequeue(&ifp.if_snd) else {
            break;
        };

        let txs = &sc.sc_txsoft[sc.sc_txsnext.get()];
        let Some(dmamap) = txs.txs_dmamap.get() else {
            panic(format_args!("{}: no tx DMA map", sc.devname()));
        };

        // SAFETY: the mbuf stays the job's (`txs_mbuf`) until `pcn_txintr` or `pcn_stop`
        // unload the map before freeing it.
        match unsafe { bus_dmamap_load_mbuf(dmat, dmamap, m0, BUS_DMA_NOWAIT) } {
            Ok(()) => {}
            Err(Errno::EFBIG) => {
                // SAFETY: as above.
                let reloaded = m_defrag(m0, M_DONTWAIT).is_ok()
                    && unsafe { bus_dmamap_load_mbuf(dmat, dmamap, m0, BUS_DMA_NOWAIT) }.is_ok();
                if !reloaded {
                    m_freem(m0);
                    continue;
                }
            }
            Err(_) => {
                m_freem(m0);
                continue;
            }
        }

        // WE ARE NOW COMMITTED TO TRANSMITTING THE PACKET.

        // Sync the DMA map.
        bus_dmamap_sync(
            dmat,
            dmamap,
            0,
            dmamap.dm_mapsize.get(),
            BUS_DMASYNC_PREWRITE,
        );

        // Initialize the transmit descriptors.
        let nsegs = dmamap.dm_nsegs.get() as usize;
        let txnext = sc.sc_txnext.get();
        let mut nexttx = txnext;
        let mut lasttx: Option<usize> = None;
        for seg in 0..nsegs {
            let s = dmamap.dm_segs()[seg].get();
            // If this is the first descriptor we're enqueueing, don't set the OWN bit just
            // yet. That could cause a race condition. We'll do it below.
            let tmd1 = LE_T1_ONES
                | if nexttx == txnext { 0 } else { LE_T1_OWN }
                | (le_bcnt(s.ds_len as u32) & LE_T1_BCNT_MASK);
            if sc.sc_swstyle.get() == LE_B20_SSTYLE_PCNETPCI3 {
                sc.set_tmd0(nexttx, 0);
                sc.set_tmd2(nexttx, (s.ds_addr as u32).to_le());
            } else {
                sc.set_tmd0(nexttx, (s.ds_addr as u32).to_le());
                sc.set_tmd2(nexttx, 0);
            }
            sc.set_tmd1(nexttx, tmd1.to_le());
            lasttx = Some(nexttx);
            nexttx = pcn_nexttx(nexttx);
        }

        let Some(lasttx) = lasttx else {
            panic(format_args!("{}: pcn_start: no segments", sc.devname()));
        };
        // Interrupt on the packet, if appropriate.
        if sc.sc_txsnext.get() & PCN_TXINTR_MASK == 0 {
            sc.set_tmd1(lasttx, sc.tmd1(lasttx) | LE_T1_LTINT.to_le());
        }

        // Set `start of packet' and `end of packet' appropriately.
        sc.set_tmd1(lasttx, sc.tmd1(lasttx) | LE_T1_ENP.to_le());
        sc.set_tmd1(txnext, sc.tmd1(txnext) | (LE_T1_OWN | LE_T1_STP).to_le());

        // Sync the descriptors we're using.
        sc.cdtxsync(txnext, nsegs, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);

        // Kick the transmitter.
        pcn_csr_write(sc, LE_CSR0, LE_C0_INEA | LE_C0_TDMD);

        // Store a pointer to the packet so we can free it later, and remember what txdirty
        // will be once the packet is done.
        txs.txs_mbuf.set(Some(m0));
        txs.txs_firstdesc.set(txnext);
        txs.txs_lastdesc.set(lasttx);

        // Advance the tx pointer.
        sc.sc_txfree.set(sc.sc_txfree.get() - nsegs as i32);
        sc.sc_txnext.set(nexttx);

        sc.sc_txsfree.set(sc.sc_txsfree.get() - 1);
        sc.sc_txsnext.set(pcn_nexttxs(sc.sc_txsnext.get()));

        // Pass the packet to any BPF listeners.
        let bpf = ifp.if_bpf.get();
        if !bpf.is_null() {
            let _ = bpf_mtap(bpf, m0, BPF_DIRECTION_OUT);
        }
    }

    if sc.sc_txfree.get() != ofree {
        // Set a watchdog timer in case the chip flakes out.
        ifp.if_timer.set(5);
    }
}

/// `pcn_watchdog`: [ifnet interface function] watchdog timer handler.
pub fn pcn_watchdog(ifp: &'static Ifnet) {
    let sc = pcn_softc(ifp);

    // Since we're not interrupting every packet, sweep up before we report an error.
    pcn_txintr(sc);

    if sc.sc_txfree.get() != PCN_NTXDESC as i32 {
        printf(format_args!(
            "{}: device timeout (txfree {} txsfree {})\n",
            sc.devname(),
            sc.sc_txfree.get(),
            sc.sc_txsfree.get()
        ));
        ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);

        // Reset the interface.
        let _ = pcn_init(ifp);
    }

    // Try to get more packets going.
    pcn_start(ifp);
}

/// `pcn_ioctl`: [ifnet interface function] handle control requests from the operator.
///
/// # Safety
///
/// As for `IfIoctlFn` (`net/if_var.rs`): `data` is the kernel copy of the request the
/// command encodes.
pub unsafe fn pcn_ioctl(ifp: &'static Ifnet, cmd: u64, data: *mut u8) -> Result<(), Errno> {
    let sc = pcn_softc(ifp);
    let mut error = Ok(());

    let s = splnet();

    match cmd {
        SIOCSIFADDR => {
            ifp.if_flags.set(ifp.if_flags.get() | IFF_UP);
            if ifp.if_flags.get() & IFF_RUNNING == 0 {
                let _ = pcn_init(ifp);
            }
        }
        SIOCSIFFLAGS => {
            if ifp.if_flags.get() & IFF_UP != 0 {
                if ifp.if_flags.get() & IFF_RUNNING != 0 {
                    error = Err(Errno::ENETRESET);
                } else {
                    let _ = pcn_init(ifp);
                }
            } else if ifp.if_flags.get() & IFF_RUNNING != 0 {
                pcn_stop(ifp, 1);
            }
        }
        SIOCSIFMEDIA | SIOCGIFMEDIA => {
            // SAFETY: SIOCSIFMEDIA carries a `struct ifreq` and SIOCGIFMEDIA a `struct
            // ifmediareq` (this function's contract).
            error = unsafe { ifmedia_ioctl(ifp, data, &sc.sc_mii.mii_media, cmd) };
        }
        _ => {
            // SAFETY: this function's contract, forwarded.
            error = unsafe { ether_ioctl(ifp, &sc.sc_arpcom, cmd, data) };
        }
    }

    if error == Err(Errno::ENETRESET) {
        error = if ifp.if_flags.get() & IFF_RUNNING != 0 {
            pcn_init(ifp)
        } else {
            Ok(())
        };
    }

    splx(s);
    error
}

/// `pcn_intr`: interrupt service routine.
pub fn pcn_intr(arg: *mut c_void) -> i32 {
    let sc = pcn_arg(arg);
    let ifp = sc.ifp();
    let mut wantinit = false;
    let mut handled = false;

    while !wantinit {
        let csr0 = pcn_csr_read(sc, LE_CSR0);
        if csr0 & LE_C0_INTR == 0 {
            break;
        }

        // ACK the bits and re-enable interrupts.
        pcn_csr_write(
            sc,
            LE_CSR0,
            csr0 & (LE_C0_INEA
                | LE_C0_BABL
                | LE_C0_MISS
                | LE_C0_MERR
                | LE_C0_RINT
                | LE_C0_TINT
                | LE_C0_IDON),
        );

        handled = true;

        if csr0 & LE_C0_RINT != 0 {
            wantinit = pcn_rxintr(sc);
        }

        if csr0 & LE_C0_TINT != 0 {
            pcn_txintr(sc);
        }

        if csr0 & LE_C0_ERR != 0 {
            if csr0 & LE_C0_BABL != 0 {
                ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);
            }
            if csr0 & LE_C0_MISS != 0 {
                ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
            }
            if csr0 & LE_C0_MERR != 0 {
                printf(format_args!("{}: memory error\n", sc.devname()));
                wantinit = true;
                break;
            }
        }

        if csr0 & LE_C0_RXON == 0 {
            printf(format_args!("{}: receiver disabled\n", sc.devname()));
            ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
            wantinit = true;
        }

        if csr0 & LE_C0_TXON == 0 {
            printf(format_args!("{}: transmitter disabled\n", sc.devname()));
            ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);
            wantinit = true;
        }
    }

    if handled {
        if wantinit {
            let _ = pcn_init(ifp);
        }

        // Try to get more packets going.
        pcn_start(ifp);
    }

    i32::from(handled)
}

/// `pcn_spnd`: suspend the chip.
pub fn pcn_spnd(sc: &PcnSoftc) {
    pcn_csr_write(sc, LE_CSR5, sc.sc_csr5.get() | LE_C5_SPND);

    for _ in 0..10000 {
        if pcn_csr_read(sc, LE_CSR5) & LE_C5_SPND != 0 {
            return;
        }
        delay(5);
    }

    printf(format_args!(
        "{}: WARNING: chip failed to enter suspended state\n",
        sc.devname()
    ));
}

/// `pcn_txintr`: helper; handle transmit interrupts.
pub fn pcn_txintr(sc: &'static PcnSoftc) {
    let ifp = sc.ifp();
    let dmat = sc.dmat();

    // Go through our Tx list and free mbufs for those frames which have been transmitted.
    let mut i = sc.sc_txsdirty.get();
    while sc.sc_txsfree.get() != PCN_TXQUEUELEN as i32 {
        let txs = &sc.sc_txsoft[i];
        let Some(map) = txs.txs_dmamap.get() else {
            panic(format_args!("{}: no tx DMA map", sc.devname()));
        };
        let lastdesc = txs.txs_lastdesc.get();

        sc.cdtxsync(
            txs.txs_firstdesc.get(),
            map.dm_nsegs.get() as usize,
            BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE,
        );

        let tmd1 = u32::from_le(sc.tmd1(lastdesc));
        if tmd1 & LE_T1_OWN != 0 {
            break;
        }

        // Slightly annoying -- we have to loop through the descriptors we've used looking for
        // ERR, since it can appear on any descriptor in the chain.
        let mut errored = false;
        let mut tmd;
        let mut j = txs.txs_firstdesc.get();
        loop {
            tmd = u32::from_le(sc.tmd1(j));
            if tmd & LE_T1_ERR != 0 {
                ifp.if_oerrors().set(ifp.if_oerrors().get() + 1);
                let tmd2 = if sc.sc_swstyle.get() == LE_B20_SSTYLE_PCNETPCI3 {
                    u32::from_le(sc.tmd0(j))
                } else {
                    u32::from_le(sc.tmd2(j))
                };
                if tmd2 & LE_T2_UFLO != 0 {
                    if sc.sc_xmtsp.get() < LE_C80_XMTSP_MAX {
                        sc.sc_xmtsp.set(sc.sc_xmtsp.get() + 1);
                        let desc = sc
                            .sc_xmtsp_desc
                            .get()
                            .and_then(|d| d.get(sc.sc_xmtsp.get() as usize))
                            .copied()
                            .unwrap_or("?");
                        printf(format_args!(
                            "{}: transmit underrun; new threshold: {}\n",
                            sc.devname(),
                            desc
                        ));
                        pcn_spnd(sc);
                        pcn_csr_write(
                            sc,
                            LE_CSR80,
                            le_c80_rcvfw(sc.sc_rcvfw.get())
                                | le_c80_xmtsp(sc.sc_xmtsp.get())
                                | le_c80_xmtfw(sc.sc_xmtfw.get()),
                        );
                        pcn_csr_write(sc, LE_CSR5, sc.sc_csr5.get());
                    } else {
                        printf(format_args!("{}: transmit underrun\n", sc.devname()));
                    }
                } else if tmd2 & LE_T2_BUFF != 0 {
                    printf(format_args!("{}: transmit buffer error\n", sc.devname()));
                }
                if tmd2 & LE_T2_LCOL != 0 {
                    ifp.if_collisions().set(ifp.if_collisions().get() + 1);
                }
                if tmd2 & LE_T2_RTRY != 0 {
                    ifp.if_collisions().set(ifp.if_collisions().get() + 16);
                }
                errored = true;
                break;
            }
            if j == lastdesc {
                break;
            }
            j = pcn_nexttx(j);
        }
        if !errored {
            if tmd1 & LE_T1_ONE != 0 {
                ifp.if_collisions().set(ifp.if_collisions().get() + 1);
            } else if tmd & LE_T1_MORE != 0 {
                // Real number is unknown.
                ifp.if_collisions().set(ifp.if_collisions().get() + 2);
            }
        }
        // next_packet:
        sc.sc_txfree.set(sc.sc_txfree.get() + map.dm_nsegs.get());
        bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTWRITE);
        bus_dmamap_unload(dmat, map);
        m_freem(txs.txs_mbuf.take());

        i = pcn_nexttxs(i);
        sc.sc_txsfree.set(sc.sc_txsfree.get() + 1);
    }

    // Update the dirty transmit buffer pointer.
    sc.sc_txsdirty.set(i);

    // If there are no more pending transmissions, cancel the watchdog timer.
    if sc.sc_txsfree.get() == PCN_TXQUEUELEN as i32 {
        ifp.if_timer.set(0);
    }

    if ifq_is_oactive(&ifp.if_snd) {
        ifq_restart(&ifp.if_snd);
    }
}

/// `pcn_rxintr`: helper; handle receive interrupts. Whether the interface needs to be
/// initialised again.
pub fn pcn_rxintr(sc: &'static PcnSoftc) -> bool {
    let ifp = sc.ifp();
    let dmat = sc.dmat();
    let ml = MbufList::new();
    let mut rv = false;

    let mut i = sc.sc_rxptr.get();
    loop {
        let rxs = &sc.sc_rxsoft[i];
        let Some(map) = rxs.rxs_dmamap.get() else {
            panic(format_args!("{}: no rx DMA map", sc.devname()));
        };

        sc.cdrxsync(i, BUS_DMASYNC_POSTREAD | BUS_DMASYNC_POSTWRITE);

        let rmd1 = u32::from_le(sc.rmd1(i));

        if rmd1 & LE_R1_OWN != 0 {
            break;
        }

        // Check for errors and make sure the packet fit into a single buffer. We have
        // structured this block of code the way it is in order to compress it into one test in
        // the common case (no error).
        if rmd1 & (LE_R1_STP | LE_R1_ENP | LE_R1_ERR) != (LE_R1_STP | LE_R1_ENP) {
            // Make sure the packet is in a single buffer.
            if rmd1 & (LE_R1_STP | LE_R1_ENP) != (LE_R1_STP | LE_R1_ENP) {
                printf(format_args!(
                    "{}: packet spilled into next buffer\n",
                    sc.devname()
                ));
                rv = true; // pcn_intr() will re-init
                if_input(ifp, &ml);
                return rv;
            }

            // If the packet had an error, simple recycle the buffer.
            if rmd1 & LE_R1_ERR != 0 {
                ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
                // If we got an overflow error, chances are there will be a CRC error. In this
                // case, just print the overflow error, and skip the others.
                if rmd1 & LE_R1_OFLO != 0 {
                    printf(format_args!("{}: overflow error\n", sc.devname()));
                } else {
                    for (bit, what) in [
                        (LE_R1_FRAM, "framing error"),
                        (LE_R1_CRC, "CRC error"),
                        (LE_R1_BUFF, "buffer error"),
                    ] {
                        if rmd1 & bit != 0 {
                            printf(format_args!("{}: {}\n", sc.devname(), what));
                        }
                    }
                }
                pcn_init_rxdesc(sc, i);
                i = pcn_nextrx(i);
                continue;
            }
        }

        bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_POSTREAD);

        // No errors; receive the packet.
        let mut len = if sc.sc_swstyle.get() == LE_B20_SSTYLE_PCNETPCI3 {
            u32::from_le(sc.rmd0(i)) & LE_R1_BCNT_MASK
        } else {
            u32::from_le(sc.rmd2(i)) & LE_R1_BCNT_MASK
        } as i32;

        // The LANCE family includes the CRC with every packet; trim it off here.
        len -= ETHER_CRC_LEN as i32;

        // If the packet is small enough to fit in a single header mbuf, allocate one and copy
        // the data into it. This greatly reduces memory consumption when we receive lots of
        // small packets.
        //
        // Otherwise, we add a new buffer to the receive chain. If this fails, we drop the
        // packet and recycle the old buffer.
        let m = if PCN_COPY_SMALL != 0 && len as usize <= crate::sys::mbuf::MHLEN - 2 {
            let Some(m) = m_gethdr(M_DONTWAIT, MT_DATA) else {
                pcn_rx_dropit(sc, i);
                i = pcn_nextrx(i);
                continue;
            };
            let Some(old) = rxs.rxs_mbuf.get() else {
                panic(format_args!("{}: no rx mbuf", sc.devname()));
            };
            m.m_data().set(m.m_data().get().wrapping_add(2));
            // SAFETY: both mbufs hold at least `len` bytes at their data pointers (`len` fits
            // a header mbuf less the 2 bytes of alignment; the receive cluster holds the
            // frame the chip stored).
            unsafe { ptr::copy_nonoverlapping(old.m_data().get(), m.m_data().get(), len as usize) };
            pcn_init_rxdesc(sc, i);
            bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_PREREAD);
            m
        } else {
            let Some(m) = rxs.rxs_mbuf.get() else {
                panic(format_args!("{}: no rx mbuf", sc.devname()));
            };
            if pcn_add_rxbuf(sc, i).is_err() {
                pcn_rx_dropit(sc, i);
                i = pcn_nextrx(i);
                continue;
            }
            m
        };

        m.m_pkthdr().len.set(len);
        m.m_len().set(len as u32);

        ml_enqueue(&ml, m);
        i = pcn_nextrx(i);
    }

    // Update the receive pointer.
    sc.sc_rxptr.set(i);
    if_input(ifp, &ml);
    rv
}

/// `pcn_rxintr`'s `dropit`: count the error and recycle the old buffer.
fn pcn_rx_dropit(sc: &'static PcnSoftc, i: usize) {
    let ifp = sc.ifp();
    let rxs = &sc.sc_rxsoft[i];

    ifp.if_ierrors().set(ifp.if_ierrors().get() + 1);
    pcn_init_rxdesc(sc, i);
    if let Some(map) = rxs.rxs_dmamap.get() {
        bus_dmamap_sync(sc.dmat(), map, 0, map.dm_mapsize.get(), BUS_DMASYNC_PREREAD);
    }
}

/// `pcn_tick`: one second timer, used to tick the MII.
pub fn pcn_tick(arg: *mut c_void) {
    let sc = pcn_arg(arg);

    let s = splnet();
    mii_tick(&sc.sc_mii);
    splx(s);

    timeout_add_sec(&sc.sc_tick_timeout, 1);
}

/// `pcn_reset`: perform a soft reset on the PCnet-PCI.
pub fn pcn_reset(sc: &PcnSoftc) {
    let (t, h) = sc.regs();

    // The PCnet-PCI chip is reset by reading from the RESET register. Note that while the
    // NE2100 LANCE boards require a write after the read, the PCnet-PCI chips do not require
    // this.
    //
    // Since we don't know if we're in 16-bit or 32-bit mode right now, issue both (it's safe)
    // in the hopes that one will succeed.
    let _ = bus_space_read_2(t, h, PCN16_RESET);
    let _ = bus_space_read_4(t, h, PCN32_RESET);

    // Wait 1ms for it to finish.
    delay(1000);

    // Select 32-bit I/O mode by issuing a 32-bit write to the RDP. Since the RAP is 0 after a
    // reset, writing a 0 to RDP is safe (since it simply clears CSR0).
    bus_space_write_4(t, h, PCN32_RDP, 0);
}

/// `pcn_init`: [ifnet interface function] initialize the interface. Must be called at
/// `splnet()`.
pub fn pcn_init(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = pcn_softc(ifp);
    // Cancel any pending I/O.
    pcn_stop(ifp, 0);

    // Reset the chip to a known state.
    pcn_reset(sc);

    // On the Am79c970, select SSTYLE 2, and SSTYLE 3 on everything else.
    //
    // XXX It'd be really nice to use SSTYLE 2 on all the chips, because the structure layout
    // is compatible with ILACC, but the burst mode is only available in SSTYLE 3, and burst
    // mode should provide some performance enhancement.
    if sc.variant().pcv_chipid == PARTID_Am79c970 {
        sc.sc_swstyle.set(LE_B20_SSTYLE_PCNETPCI2);
    } else {
        sc.sc_swstyle.set(LE_B20_SSTYLE_PCNETPCI3);
    }
    pcn_bcr_write(sc, LE_BCR20, sc.sc_swstyle.get());

    // Initialize the transmit descriptor ring.
    sc.clear_txdescs();
    sc.cdtxsync(0, PCN_NTXDESC, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
    sc.sc_txfree.set(PCN_NTXDESC as i32);
    sc.sc_txnext.set(0);

    // Initialize the transmit job descriptors.
    for txs in sc.sc_txsoft.iter() {
        txs.txs_mbuf.set(None);
    }
    sc.sc_txsfree.set(PCN_TXQUEUELEN as i32);
    sc.sc_txsnext.set(0);
    sc.sc_txsdirty.set(0);

    // Initialize the receive descriptor and receive job descriptor rings.
    for i in 0..PCN_NRXDESC {
        let rxs = &sc.sc_rxsoft[i];
        if rxs.rxs_mbuf.get().is_none() {
            if let Err(e) = pcn_add_rxbuf(sc, i) {
                printf(format_args!(
                    "{}: unable to allocate or map rx buffer {}, error = {}\n",
                    sc.devname(),
                    i,
                    e as i32
                ));
                // XXX Should attempt to run with fewer receive XXX buffers instead of just
                // failing.
                pcn_rxdrain(sc);
                return pcn_init_out(sc, Err(e));
            }
        } else {
            pcn_init_rxdesc(sc, i);
        }
    }
    sc.sc_rxptr.set(0);

    // Initialize MODE for the initialization block.
    sc.sc_mode.set(0);

    // If we have MII, simply select MII in the MODE register, and clear ASEL. Otherwise, let
    // ASEL stand (for now), and leave PORTSEL alone (it is ignored with ASEL is set).
    if sc.sc_flags.get() & PCN_F_HAS_MII != 0 {
        pcn_bcr_write(sc, LE_BCR2, pcn_bcr_read(sc, LE_BCR2) & !LE_B2_ASEL);
        sc.sc_mode
            .set(sc.sc_mode.get() | le_c15_portsel(PORTSEL_MII));

        // Disable MII auto-negotiation. We handle that in our own MII layer.
        pcn_bcr_write(sc, LE_BCR32, pcn_bcr_read(sc, LE_BCR32) | LE_B32_DANAS);
    }

    // Set the multicast filter in the init block.
    pcn_set_filter(sc);

    // Set the Tx and Rx descriptor ring addresses in the init block, the TLEN and RLEN other
    // fields of the init block MODE register.
    let mut initblock = sc.initblock();
    initblock.init_rdra = (sc.cdrxaddr(0) as u32).to_le();
    initblock.init_tdra = (sc.cdtxaddr(0) as u32).to_le();
    initblock.init_mode = (sc.sc_mode.get()
        | ((PCN_NTXDESC.trailing_zeros()) << 28)
        | ((PCN_NRXDESC.trailing_zeros()) << 20))
        .to_le();

    // Set the station address in the init block.
    // SAFETY: `if_sadl` is set by `ether_ifattach` in `pcn_attach`; `lladdr` only computes
    // the address of the link-level address, `ETHER_ADDR_LEN` bytes long, which follows the
    // name in the `sockaddr_dl` allocation.
    let enaddr: [u8; ETHER_ADDR_LEN] = unsafe {
        let p = crate::net::if_dl::lladdr(ifp.if_sadl.get());
        core::array::from_fn(|i| *p.add(i))
    };
    initblock.init_padr[0] = u32::from(enaddr[0])
        .wrapping_add(u32::from(enaddr[1]) << 8)
        .wrapping_add(u32::from(enaddr[2]) << 16)
        .wrapping_add(u32::from(enaddr[3]) << 24)
        .to_le();
    initblock.init_padr[1] = (u32::from(enaddr[4]) | (u32::from(enaddr[5]) << 8)).to_le();
    sc.set_initblock(initblock);

    // Initialize CSR3.
    pcn_csr_write(sc, LE_CSR3, LE_C3_MISSM | LE_C3_IDONM | LE_C3_DXSUFLO);

    // Initialize CSR4.
    pcn_csr_write(
        sc,
        LE_CSR4,
        LE_C4_DMAPLUS | LE_C4_APAD_XMT | LE_C4_MFCOM | LE_C4_RCVCCOM | LE_C4_TXSTRTM,
    );

    // Initialize CSR5.
    sc.sc_csr5.set(LE_C5_LTINTEN | LE_C5_SINTE);
    pcn_csr_write(sc, LE_CSR5, sc.sc_csr5.get());

    // If we have an Am79c971 or greater, initialize CSR7.
    //
    // XXX Might be nice to use the MII auto-poll interrupt someday.
    let chipid = sc.variant().pcv_chipid;
    if chipid != PARTID_Am79c970 && chipid != PARTID_Am79c970A {
        // (Not available on the Am79c970 and Am79c970A.)
        pcn_csr_write(sc, LE_CSR7, LE_C7_FASTSPNDE);
    }

    // On the Am79c970A and greater, initialize BCR18 to enable burst mode.
    //
    // Also enable the "no underflow" option on the Am79c971 and higher, which prevents the
    // chip from generating transmit underflows, yet sill provides decent performance. Note if
    // chip is not connected to external SRAM, then we still have to handle underflow errors
    // (the NOUFLO bit is ignored in that case).
    let mut reg = pcn_bcr_read(sc, LE_BCR18);
    if chipid == PARTID_Am79c970A {
        reg |= LE_B18_BREADE | LE_B18_BWRITE;
    } else if chipid != PARTID_Am79c970 {
        reg |= LE_B18_BREADE | LE_B18_BWRITE | LE_B18_NOUFLO;
    }
    pcn_bcr_write(sc, LE_BCR18, reg);

    // Initialize CSR80 (FIFO thresholds for Tx and Rx).
    pcn_csr_write(
        sc,
        LE_CSR80,
        le_c80_rcvfw(sc.sc_rcvfw.get())
            | le_c80_xmtsp(sc.sc_xmtsp.get())
            | le_c80_xmtfw(sc.sc_xmtfw.get()),
    );

    // Send the init block to the chip, and wait for it to be processed.
    sc.cdinitsync(BUS_DMASYNC_PREWRITE);
    pcn_csr_write(sc, LE_CSR1, (sc.cdinitaddr() as u32) & 0xffff);
    pcn_csr_write(sc, LE_CSR2, ((sc.cdinitaddr() as u32) >> 16) & 0xffff);
    pcn_csr_write(sc, LE_CSR0, LE_C0_INIT);
    delay(100);
    let mut i = 0;
    while i < 10000 {
        if pcn_csr_read(sc, LE_CSR0) & LE_C0_IDON != 0 {
            break;
        }
        delay(10);
        i += 1;
    }
    sc.cdinitsync(BUS_DMASYNC_POSTWRITE);
    if i == 10000 {
        printf(format_args!(
            "{}: timeout processing init block\n",
            sc.devname()
        ));
        return pcn_init_out(sc, Err(Errno::EIO));
    }

    // Set the media.
    if let Some(cb) = sc.sc_mii.mii_media.ifm_change_cb.get() {
        let _ = cb(ifp);
    }

    // Enable interrupts and external activity (and ACK IDON).
    pcn_csr_write(sc, LE_CSR0, LE_C0_INEA | LE_C0_STRT | LE_C0_IDON);

    if sc.sc_flags.get() & PCN_F_HAS_MII != 0 {
        // Start the one second MII clock.
        timeout_add_sec(&sc.sc_tick_timeout, 1);
    }

    // ...all done!
    ifp.if_flags.set(ifp.if_flags.get() | IFF_RUNNING);
    ifq_clr_oactive(&ifp.if_snd);

    pcn_init_out(sc, Ok(()))
}

/// `pcn_init`'s `out:` label.
fn pcn_init_out(sc: &PcnSoftc, error: Result<(), Errno>) -> Result<(), Errno> {
    if error.is_err() {
        printf(format_args!("{}: interface not running\n", sc.devname()));
    }
    error
}

/// `pcn_rxdrain`: drain the receive queue.
pub fn pcn_rxdrain(sc: &PcnSoftc) {
    let dmat = sc.dmat();

    for rxs in sc.sc_rxsoft.iter() {
        if let Some(m) = rxs.rxs_mbuf.take() {
            if let Some(map) = rxs.rxs_dmamap.get() {
                bus_dmamap_unload(dmat, map);
            }
            m_freem(m);
        }
    }
}

/// `pcn_stop`: [ifnet interface function] stop transmission on the interface.
pub fn pcn_stop(ifp: &'static Ifnet, disable: i32) {
    let sc = pcn_softc(ifp);
    let dmat = sc.dmat();

    if sc.sc_flags.get() & PCN_F_HAS_MII != 0 {
        // Stop the one second clock.
        timeout_del(&sc.sc_tick_timeout);

        // Down the MII.
        mii_down(&sc.sc_mii);
    }

    // Mark the interface as down and cancel the watchdog timer.
    ifp.if_flags.set(ifp.if_flags.get() & !IFF_RUNNING);
    ifq_clr_oactive(&ifp.if_snd);
    ifp.if_timer.set(0);

    // Stop the chip.
    pcn_csr_write(sc, LE_CSR0, LE_C0_STOP);

    // Release any queued transmit buffers.
    for txs in sc.sc_txsoft.iter() {
        if let Some(m) = txs.txs_mbuf.take() {
            if let Some(map) = txs.txs_dmamap.get() {
                bus_dmamap_unload(dmat, map);
            }
            m_freem(m);
        }
    }

    if disable != 0 {
        pcn_rxdrain(sc);
    }
}

/// `pcn_add_rxbuf`: add a receive buffer to the indicated descriptor.
pub fn pcn_add_rxbuf(sc: &PcnSoftc, idx: usize) -> Result<(), Errno> {
    let rxs = &sc.sc_rxsoft[idx];
    let dmat = sc.dmat();

    let Some(m) = m_gethdr(M_DONTWAIT, MT_DATA) else {
        return Err(Errno::ENOBUFS);
    };

    let Some(m) = m_clget(Some(m), M_DONTWAIT, MCLBYTES as u32) else {
        m_freem(m);
        return Err(Errno::ENOBUFS);
    };

    let Some(map) = rxs.rxs_dmamap.get() else {
        panic(format_args!("{}: no rx DMA map", sc.devname()));
    };

    if rxs.rxs_mbuf.get().is_some() {
        bus_dmamap_unload(dmat, map);
    }

    rxs.rxs_mbuf.set(Some(m));

    // SAFETY: the cluster stays the slot's (`rxs_mbuf`) until `pcn_rxintr` or `pcn_rxdrain`
    // hand it on or free it, after unloading the map.
    let error = unsafe {
        bus_dmamap_load(
            dmat,
            map,
            m.m_ext().ext_buf.get(),
            m.m_ext().ext_size.get() as BusSize,
            None,
            BUS_DMA_READ | BUS_DMA_NOWAIT,
        )
    };
    if let Err(error) = error {
        printf(format_args!(
            "{}: can't load rx DMA map {}, error = {}\n",
            sc.devname(),
            idx,
            error as i32
        ));
        panic(format_args!("pcn_add_rxbuf"));
    }

    bus_dmamap_sync(dmat, map, 0, map.dm_mapsize.get(), BUS_DMASYNC_PREREAD);

    pcn_init_rxdesc(sc, idx);

    Ok(())
}

/// `PCN_INIT_RXDESC(sc, x)`: give receive descriptor `x` and its buffer to the chip.
fn pcn_init_rxdesc(sc: &PcnSoftc, x: usize) {
    let rxs = &sc.sc_rxsoft[x];
    let (Some(m), Some(map)) = (rxs.rxs_mbuf.get(), rxs.rxs_dmamap.get()) else {
        panic(format_args!(
            "{}: rx slot {} has no buffer",
            sc.devname(),
            x
        ));
    };

    // Note: We scoot the packet forward 2 bytes in the buffer so that the payload after the
    // Ethernet header is aligned to a 4-byte boundary.
    m.m_data().set(m.m_ext().ext_buf.get().wrapping_add(2));

    let addr = (map.dm_segs()[0].get().ds_addr as u32)
        .wrapping_add(2)
        .to_le();
    if sc.sc_swstyle.get() == LE_B20_SSTYLE_PCNETPCI3 {
        sc.set_rmd2(x, addr);
        sc.set_rmd0(x, 0);
    } else {
        sc.set_rmd2(x, 0);
        sc.set_rmd0(x, addr);
    }
    sc.set_rmd1(
        x,
        (LE_R1_OWN | LE_R1_ONES | (le_bcnt(MCLBYTES as u32 - 2) & LE_R1_BCNT_MASK)).to_le(),
    );
    sc.cdrxsync(x, BUS_DMASYNC_PREREAD | BUS_DMASYNC_PREWRITE);
}

/// `pcn_set_filter`'s hash: pass the multicast address through a CRC generator, and use the
/// high order 6 bits as an index into the 64-bit logical address filter. The high order bits
/// select the word, while the rest of the bits select the bit within the word.
pub fn pcn_hash_multi(ladrf: &mut [u16; 4], addr: &[u8]) {
    let mut crc = ether_crc32_le(&addr[..ETHER_ADDR_LEN]);

    // Just want the 6 most significant bits.
    crc >>= 26;

    // Set the corresponding bit in the filter.
    ladrf[(crc >> 4) as usize] |= (1u16 << (crc & 0xf)).to_le();
}

/// `pcn_set_filter`: set up the receive filter.
pub fn pcn_set_filter(sc: &PcnSoftc) {
    let ac = &sc.sc_arpcom;
    let ifp = &ac.ac_if;
    let mut initblock = sc.initblock();

    ifp.if_flags.set(ifp.if_flags.get() & !IFF_ALLMULTI);

    if ifp.if_flags.get() & IFF_PROMISC != 0 || ac.ac_multirangecnt.get() > 0 {
        ifp.if_flags.set(ifp.if_flags.get() | IFF_ALLMULTI);
        if ifp.if_flags.get() & IFF_PROMISC != 0 {
            sc.sc_mode.set(sc.sc_mode.get() | LE_C15_PROM);
        }
        initblock.init_ladrf = [0xffff; 4];
    } else {
        initblock.init_ladrf = [0; 4];

        // Set up the multicast address filter by passing all multicast addresses through a
        // CRC generator, and then using the high order 6 bits as an index into the 64-bit
        // logical address filter.
        let mut step = EtherMultistep { e_enm: None };
        let mut enm = ether_first_multi(&mut step, ac);
        while let Some(e) = enm {
            pcn_hash_multi(&mut initblock.init_ladrf, &e.enm_addrlo);
            enm = ether_next_multi(&mut step);
        }
    }

    sc.set_initblock(initblock);
}

/// `pcn_79c970_mediainit`: initialize media for the Am79c970.
pub fn pcn_79c970_mediainit(sc: &'static PcnSoftc) {
    let media = &sc.sc_mii.mii_media;
    let a = sc.variant().pcv_chipid == PARTID_Am79c970A;

    ifmedia_init(
        media,
        IFM_IMASK,
        pcn_79c970_mediachange,
        pcn_79c970_mediastatus,
    );

    ifmedia_add(
        media,
        IFM_ETHER | IFM_10_5,
        PORTSEL_AUI as i32,
        ptr::null_mut(),
    );
    if a {
        ifmedia_add(
            media,
            IFM_ETHER | IFM_10_5 | IFM_FDX,
            PORTSEL_AUI as i32,
            ptr::null_mut(),
        );
    }

    ifmedia_add(
        media,
        IFM_ETHER | IFM_10_T,
        PORTSEL_10T as i32,
        ptr::null_mut(),
    );
    if a {
        ifmedia_add(
            media,
            IFM_ETHER | IFM_10_T | IFM_FDX,
            PORTSEL_10T as i32,
            ptr::null_mut(),
        );
    }

    ifmedia_add(media, IFM_ETHER | IFM_AUTO, 0, ptr::null_mut());
    if a {
        ifmedia_add(media, IFM_ETHER | IFM_AUTO | IFM_FDX, 0, ptr::null_mut());
    }

    ifmedia_set(media, IFM_ETHER | IFM_AUTO);
}

/// `pcn_79c970_mediastatus`: [ifmedia interface function] get the current interface media
/// status (Am79c970 version).
pub fn pcn_79c970_mediastatus(ifp: &'static Ifnet, ifmr: &mut Ifmediareq) {
    let sc = pcn_softc(ifp);

    // The currently selected media is always the active media. Note: We have no way to
    // determine what media the AUTO process picked.
    ifmr.ifm_active = sc.sc_mii.mii_media.ifm_media.get();
}

/// `pcn_79c970_mediachange`: [ifmedia interface function] set hardware to newly-selected
/// media (Am79c970 version).
pub fn pcn_79c970_mediachange(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = pcn_softc(ifp);
    let media = sc.sc_mii.mii_media.ifm_media.get();

    if ifm_subtype(media) == IFM_AUTO {
        // CSR15:PORTSEL doesn't matter. Just set BCR2:ASEL.
        let mut reg = pcn_bcr_read(sc, LE_BCR2);
        reg |= LE_B2_ASEL;
        pcn_bcr_write(sc, LE_BCR2, reg);
    } else {
        // Clear BCR2:ASEL and set the new CSR15:PORTSEL value.
        let mut reg = pcn_bcr_read(sc, LE_BCR2);
        reg &= !LE_B2_ASEL;
        pcn_bcr_write(sc, LE_BCR2, reg);

        reg = pcn_csr_read(sc, LE_CSR15);
        reg = (reg & !le_c15_portsel(PORTSEL_MASK)) | le_c15_portsel(sc.sc_mii.cur_data());
        pcn_csr_write(sc, LE_CSR15, reg);
    }

    if media & IFM_FDX != 0 {
        let mut reg = LE_B9_FDEN;
        if ifm_subtype(media) == IFM_10_5 {
            reg |= LE_B9_AUIFD;
        }
        pcn_bcr_write(sc, LE_BCR9, reg);
    } else {
        pcn_bcr_write(sc, LE_BCR9, 0);
    }

    Ok(())
}

/// `pcn_79c971_mediainit`: initialize media for the Am79c971.
pub fn pcn_79c971_mediainit(sc: &'static PcnSoftc) {
    let ifp = sc.ifp();
    let mii = &sc.sc_mii;

    // We have MII.
    sc.sc_flags.set(sc.sc_flags.get() | PCN_F_HAS_MII);

    // The built-in 10BASE-T interface is mapped to the MII on the PCNet-FAST. Unfortunately,
    // there's no EEPROM word that tells us which PHY to use. This driver used to ignore all
    // but the first PHY to answer, but this code was removed to support multiple external
    // PHYs. As the default instance will be the first one to answer, no harm is done by
    // letting the possibly non-connected internal PHY show up.

    // Initialize our media structures and probe the MII.
    mii.mii_ifp.set(Some(ifp));
    mii.mii_readreg.set(Some(pcn_mii_readreg));
    mii.mii_writereg.set(Some(pcn_mii_writereg));
    mii.mii_statchg.set(Some(pcn_mii_statchg));
    ifmedia_init(
        &mii.mii_media,
        0,
        pcn_79c971_mediachange,
        pcn_79c971_mediastatus,
    );

    mii_attach(
        &sc.sc_dev,
        mii,
        0xffff_ffff_u32 as i32,
        MII_PHY_ANY,
        MII_OFFSET_ANY,
        0,
    );
    if mii.mii_phys.first().is_none() {
        ifmedia_add(&mii.mii_media, IFM_ETHER | IFM_NONE, 0, ptr::null_mut());
        ifmedia_set(&mii.mii_media, IFM_ETHER | IFM_NONE);
    } else {
        ifmedia_set(&mii.mii_media, IFM_ETHER | IFM_AUTO);
    }
}

/// `pcn_79c971_mediastatus`: [ifmedia interface function] get the current interface media
/// status (Am79c971 version).
pub fn pcn_79c971_mediastatus(ifp: &'static Ifnet, ifmr: &mut Ifmediareq) {
    let sc = pcn_softc(ifp);

    mii_pollstat(&sc.sc_mii);
    ifmr.ifm_status = sc.sc_mii.mii_media_status.get();
    ifmr.ifm_active = sc.sc_mii.mii_media_active.get();
}

/// `pcn_79c971_mediachange`: [ifmedia interface function] set hardware to newly-selected
/// media (Am79c971 version).
pub fn pcn_79c971_mediachange(ifp: &'static Ifnet) -> Result<(), Errno> {
    let sc = pcn_softc(ifp);

    if ifp.if_flags.get() & IFF_UP != 0 {
        let _ = mii_mediachg(&sc.sc_mii);
    }
    Ok(())
}

/// `pcn_mii_readreg`: [mii interface function] read a PHY register on the MII.
pub fn pcn_mii_readreg(self_: &Device, phy: i32, reg: i32) -> i32 {
    let sc = pcn_dev_sc(self_);

    pcn_bcr_write(sc, LE_BCR33, reg as u32 | ((phy as u32) << PHYAD_SHIFT));
    let rv = pcn_bcr_read(sc, LE_BCR34) & crate::dev::ic::lancereg::LE_B34_MIIMD;
    if rv == 0xffff {
        return 0;
    }

    rv as i32
}

/// `pcn_mii_writereg`: [mii interface function] write a PHY register on the MII.
pub fn pcn_mii_writereg(self_: &Device, phy: i32, reg: i32, val: i32) {
    let sc = pcn_dev_sc(self_);

    pcn_bcr_write(sc, LE_BCR33, reg as u32 | ((phy as u32) << PHYAD_SHIFT));
    pcn_bcr_write(sc, LE_BCR34, val as u32);
}

/// `pcn_mii_statchg`: [mii interface function] callback from MII layer when media changes.
pub fn pcn_mii_statchg(self_: &Device) {
    let sc = pcn_dev_sc(self_);

    if sc.sc_mii.mii_media_active.get() & IFM_FDX != 0 {
        pcn_bcr_write(sc, LE_BCR9, LE_B9_FDEN);
    } else {
        pcn_bcr_write(sc, LE_BCR9, 0);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_found_by_part_id_and_unknown_is_a_fast_chip() {
        assert_eq!(pcn_lookup_variant(PARTID_Am79c970A).pcv_desc, "Am79c970A");
        assert_eq!(pcn_lookup_variant(PARTID_Am79c973).pcv_desc, "Am79c973");
        assert_eq!(pcn_lookup_variant(0x1234).pcv_desc, "Unknown");
        assert_eq!(pcn_lookup_variant(0).pcv_desc, "Unknown");
    }

    #[test]
    fn qemu_chip_id_decodes_as_an_am79c970a() {
        // QEMU's pcnet: CSR88/89 give 0x0262_1003 (version 0, part id 0x2621, manufacturer
        // AMD 0x001).
        let chipid = 0x0262_1003;
        assert_eq!(chipid_partid(chipid), PARTID_Am79c970A);
        assert_eq!(chipid_ver(chipid), 0);
    }

    #[test]
    fn rings_wrap_and_offsets_follow_the_layout() {
        assert_eq!(pcn_nexttx(PCN_NTXDESC - 1), 0);
        assert_eq!(pcn_nexttxs(PCN_TXQUEUELEN - 1), 0);
        assert_eq!(pcn_nextrx(PCN_NRXDESC - 1), 0);
        assert_eq!(pcn_cdtxoff(1), 16);
        assert_eq!(pcn_cdrxoff(0), PCN_NTXDESC * 16);
        assert_eq!(PCN_CDINITOFF, (PCN_NTXDESC + PCN_NRXDESC) * 16);
        // TLEN and RLEN of the init block: log2 of the ring sizes.
        assert_eq!(PCN_NTXDESC.trailing_zeros(), 9);
        assert_eq!(PCN_NRXDESC.trailing_zeros(), 7);
    }

    #[test]
    fn multicast_hash_sets_one_bit_of_the_64_bit_filter() {
        let mut ladrf = [0u16; 4];
        // 01:00:5e:00:00:01, all hosts.
        pcn_hash_multi(&mut ladrf, &[0x01, 0x00, 0x5e, 0x00, 0x00, 0x01]);
        let set: u32 = ladrf.iter().map(|w| w.count_ones()).sum();
        assert_eq!(set, 1);
        let crc = ether_crc32_le(&[0x01, 0x00, 0x5e, 0x00, 0x00, 0x01]) >> 26;
        assert_eq!(ladrf[(crc >> 4) as usize], 1u16 << (crc & 0xf));
    }

    #[test]
    fn byte_counts_are_twos_complement_in_twelve_bits() {
        assert_eq!(le_bcnt(60) & LE_T1_BCNT_MASK, 0x1000 - 60);
        assert_eq!(
            le_bcnt(MCLBYTES as u32 - 2) & LE_R1_BCNT_MASK,
            0x1000 - 2046
        );
        assert_eq!(LE_C80_XMTSP_MAX, 3);
    }
}
/* </TESTS> */
