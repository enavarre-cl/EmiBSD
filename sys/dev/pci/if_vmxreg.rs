/*	$OpenBSD: if_vmxreg.h,v 1.10 2024/06/07 08:44:25 jan Exp $	*/
/* <LICENSES> */
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

//! `<dev/pci/if_vmxreg.h>`: the registers of VMware's VMXNET3 virtual NIC, its descriptor
//! formats and the structures the driver shares with the device.
//!
//! Upstream: sys/dev/pci/if_vmxreg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The register offsets are `BusSize` (`bus_space(9)`'s offset type), every other value
//!   a `u32` (the C's are untyped macros). The function-like macros `VMXNET3_BAR0_IMASK`,
//!   `VMXNET3_BAR0_TXH`, `VMXNET3_BAR0_RXH1` and `VMXNET3_BAR0_RXH2` are `const fn`s with
//!   lower-case names.
//! - The `__packed` enums `UPT1_TxStats` and `UPT1_RxStats` are `#[repr(u8)]` enums
//!   with the C's names (`Upt1TxStats::UPT1_TxStat_TSO_packets`, ...).
//! - The `__packed` structures are `#[repr(C)]`: every member falls on its natural offset,
//!   so they have the C's packed layout, which compile-time checks below assert (sizes and
//!   the offsets the device reads). Their alignment is the natural one (8, 4 or 2), which
//!   the driver's DMA allocations give them. The descriptors are plain data read and
//!   written whole and volatile by the driver (`if_vmx.rs`); `struct vmxnet3_driver_shared`'s
//!   anonymous `rss`, `pm` and `plugin` structure is [`Vmxnet3SharedConf`].
//! - The `ZERO` constants stand for the C's `bzero` of these structures.

use crate::machine::bus::BusSize;

/// `enum UPT1_TxStats`: the transmit counters of `struct vmxnet3_txq_shared`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim
pub enum Upt1TxStats {
    /// `UPT1_TxStat_TSO_packets`.
    UPT1_TxStat_TSO_packets,
    /// `UPT1_TxStat_TSO_bytes`.
    UPT1_TxStat_TSO_bytes,
    /// `UPT1_TxStat_ucast_packets`.
    UPT1_TxStat_ucast_packets,
    /// `UPT1_TxStat_ucast_bytes`.
    UPT1_TxStat_ucast_bytes,
    /// `UPT1_TxStat_mcast_packets`.
    UPT1_TxStat_mcast_packets,
    /// `UPT1_TxStat_mcast_bytes`.
    UPT1_TxStat_mcast_bytes,
    /// `UPT1_TxStat_bcast_packets`.
    UPT1_TxStat_bcast_packets,
    /// `UPT1_TxStat_bcast_bytes`.
    UPT1_TxStat_bcast_bytes,
    /// `UPT1_TxStat_error`.
    UPT1_TxStat_error,
    /// `UPT1_TxStat_discard`.
    UPT1_TxStat_discard,

    /// `UPT1_TxStats_count`.
    UPT1_TxStats_count,
}

/// `enum UPT1_RxStats`: the receive counters of `struct vmxnet3_rxq_shared`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)] // OpenBSD names, verbatim
pub enum Upt1RxStats {
    /// `UPT1_RXStat_LRO_packets`.
    UPT1_RXStat_LRO_packets,
    /// `UPT1_RXStat_LRO_bytes`.
    UPT1_RXStat_LRO_bytes,
    /// `UPT1_RXStat_ucast_packets`.
    UPT1_RXStat_ucast_packets,
    /// `UPT1_RXStat_ucast_bytes`.
    UPT1_RXStat_ucast_bytes,
    /// `UPT1_RXStat_mcast_packets`.
    UPT1_RXStat_mcast_packets,
    /// `UPT1_RXStat_mcast_bytes`.
    UPT1_RXStat_mcast_bytes,
    /// `UPT1_RXStat_bcast_packets`.
    UPT1_RXStat_bcast_packets,
    /// `UPT1_RXStat_bcast_bytes`.
    UPT1_RXStat_bcast_bytes,
    /// `UPT1_RXStat_nobuffer`.
    UPT1_RXStat_nobuffer,
    /// `UPT1_RXStat_error`.
    UPT1_RXStat_error,

    /// `UPT1_RxStats_count`.
    UPT1_RxStats_count,
}

pub use Upt1RxStats::*;
pub use Upt1TxStats::*;

// interrupt moderation levels

/// `UPT1_IMOD_NONE`: no moderation.
pub const UPT1_IMOD_NONE: u8 = 0;
/// `UPT1_IMOD_HIGHEST`: least interrupts.
pub const UPT1_IMOD_HIGHEST: u8 = 7;
/// `UPT1_IMOD_ADAPTIVE`: adaptive interrupt moderation.
pub const UPT1_IMOD_ADAPTIVE: u8 = 8;

// hardware features

/// `UPT1_F_CSUM`: Rx checksum verification.
pub const UPT1_F_CSUM: u64 = 0x0001;
/// `UPT1_F_RSS`: receive side scaling.
pub const UPT1_F_RSS: u64 = 0x0002;
/// `UPT1_F_VLAN`: VLAN tag stripping.
pub const UPT1_F_VLAN: u64 = 0x0004;
/// `UPT1_F_LRO`: large receive offloading.
pub const UPT1_F_LRO: u64 = 0x0008;

/// `VMXNET3_BAR0_IMASK(irq)`: interrupt mask.
pub const fn vmxnet3_bar0_imask(irq: u32) -> BusSize {
    irq as BusSize * 8
}
/// `VMXNET3_BAR0_TXH(q)`: Tx head.
pub const fn vmxnet3_bar0_txh(q: u32) -> BusSize {
    0x600 + q as BusSize * 8
}
/// `VMXNET3_BAR0_RXH1(q)`: ring1 Rx head.
pub const fn vmxnet3_bar0_rxh1(q: u32) -> BusSize {
    0x800 + q as BusSize * 8
}
/// `VMXNET3_BAR0_RXH2(q)`: ring2 Rx head.
pub const fn vmxnet3_bar0_rxh2(q: u32) -> BusSize {
    0xa00 + q as BusSize * 8
}
/// `VMXNET3_BAR1_VRRS`: VMXNET3 revision report selection.
pub const VMXNET3_BAR1_VRRS: BusSize = 0x000;
/// `VMXNET3_BAR1_UVRS`: UPT version report selection.
pub const VMXNET3_BAR1_UVRS: BusSize = 0x008;
/// `VMXNET3_BAR1_DSL`: driver shared address low.
pub const VMXNET3_BAR1_DSL: BusSize = 0x010;
/// `VMXNET3_BAR1_DSH`: driver shared address high.
pub const VMXNET3_BAR1_DSH: BusSize = 0x018;
/// `VMXNET3_BAR1_CMD`: command.
pub const VMXNET3_BAR1_CMD: BusSize = 0x020;
/// `VMXNET3_BAR1_MACL`: MAC address low.
pub const VMXNET3_BAR1_MACL: BusSize = 0x028;
/// `VMXNET3_BAR1_MACH`: MAC address high.
pub const VMXNET3_BAR1_MACH: BusSize = 0x030;
/// `VMXNET3_BAR1_INTR`: interrupt status.
pub const VMXNET3_BAR1_INTR: BusSize = 0x038;
/// `VMXNET3_BAR1_EVENT`: event status.
pub const VMXNET3_BAR1_EVENT: BusSize = 0x040;

/// `VMXNET3_CMD_ENABLE`: enable VMXNET3.
pub const VMXNET3_CMD_ENABLE: u32 = 0xcafe0000;
/// `VMXNET3_CMD_DISABLE`: disable VMXNET3.
pub const VMXNET3_CMD_DISABLE: u32 = 0xcafe0001;
/// `VMXNET3_CMD_RESET`: reset device.
pub const VMXNET3_CMD_RESET: u32 = 0xcafe0002;
/// `VMXNET3_CMD_SET_RXMODE`: set interface flags.
pub const VMXNET3_CMD_SET_RXMODE: u32 = 0xcafe0003;
/// `VMXNET3_CMD_SET_FILTER`: set address filter.
pub const VMXNET3_CMD_SET_FILTER: u32 = 0xcafe0004;
/// `VMXNET3_CMD_SET_FEATURE`: set features.
pub const VMXNET3_CMD_SET_FEATURE: u32 = 0xcafe0009;
/// `VMXNET3_CMD_GET_STATUS`: get queue errors.
pub const VMXNET3_CMD_GET_STATUS: u32 = 0xf00d0000;
/// `VMXNET3_CMD_GET_STATS`.
pub const VMXNET3_CMD_GET_STATS: u32 = 0xf00d0001;
/// `VMXNET3_CMD_GET_LINK`: get link status.
pub const VMXNET3_CMD_GET_LINK: u32 = 0xf00d0002;
/// `VMXNET3_CMD_GET_MACL`.
pub const VMXNET3_CMD_GET_MACL: u32 = 0xf00d0003;
/// `VMXNET3_CMD_GET_MACH`.
pub const VMXNET3_CMD_GET_MACH: u32 = 0xf00d0004;
/// `VMXNET3_CMD_GET_INTRCFG`: get interrupt config.
pub const VMXNET3_CMD_GET_INTRCFG: u32 = 0xf00d0008;
/// `VMXNET3_INTRCFG_TYPE_SHIFT`.
pub const VMXNET3_INTRCFG_TYPE_SHIFT: u32 = 0;
/// `VMXNET3_INTRCFG_TYPE_MASK`.
pub const VMXNET3_INTRCFG_TYPE_MASK: u32 = 0x3 << VMXNET3_INTRCFG_TYPE_SHIFT;
/// `VMXNET3_INTRCFG_TYPE_AUTO`.
pub const VMXNET3_INTRCFG_TYPE_AUTO: u32 = 0x0 << VMXNET3_INTRCFG_TYPE_SHIFT;
/// `VMXNET3_INTRCFG_TYPE_INTX`.
pub const VMXNET3_INTRCFG_TYPE_INTX: u32 = 0x1 << VMXNET3_INTRCFG_TYPE_SHIFT;
/// `VMXNET3_INTRCFG_TYPE_MSI`.
pub const VMXNET3_INTRCFG_TYPE_MSI: u32 = 0x2 << VMXNET3_INTRCFG_TYPE_SHIFT;
/// `VMXNET3_INTRCFG_TYPE_MSIX`.
pub const VMXNET3_INTRCFG_TYPE_MSIX: u32 = 0x3 << VMXNET3_INTRCFG_TYPE_SHIFT;
/// `VMXNET3_INTRCFG_MODE_SHIFT`.
pub const VMXNET3_INTRCFG_MODE_SHIFT: u32 = 2;
/// `VMXNET3_INTRCFG_MODE_MASK`.
pub const VMXNET3_INTRCFG_MODE_MASK: u32 = 0x3 << VMXNET3_INTRCFG_MODE_SHIFT;
/// `VMXNET3_INTRCFG_MODE_AUTO`.
pub const VMXNET3_INTRCFG_MODE_AUTO: u32 = 0x0 << VMXNET3_INTRCFG_MODE_SHIFT;
/// `VMXNET3_INTRCFG_MODE_ACTIVE`.
pub const VMXNET3_INTRCFG_MODE_ACTIVE: u32 = 0x1 << VMXNET3_INTRCFG_MODE_SHIFT;
/// `VMXNET3_INTRCFG_MODE_LAZY`.
pub const VMXNET3_INTRCFG_MODE_LAZY: u32 = 0x2 << VMXNET3_INTRCFG_MODE_SHIFT;

/// `VMXNET3_DMADESC_ALIGN`.
pub const VMXNET3_DMADESC_ALIGN: u32 = 128;

// All descriptors are in little-endian format.

/// `struct vmxnet3_txdesc`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vmxnet3Txdesc {
    /// `tx_addr`.
    pub tx_addr: u64,
    /// `tx_word2`.
    pub tx_word2: u32,
    /// `tx_word3`.
    pub tx_word3: u32,
}

/// `VMXNET3_TX_LEN_M`.
pub const VMXNET3_TX_LEN_M: u32 = 0x00003fff;
/// `VMXNET3_TX_LEN_S`.
pub const VMXNET3_TX_LEN_S: u32 = 0;
/// `VMXNET3_TX_GEN_M`: generation.
pub const VMXNET3_TX_GEN_M: u32 = 0x00000001;
/// `VMXNET3_TX_GEN_S`.
pub const VMXNET3_TX_GEN_S: u32 = 14;
/// `VMXNET3_TX_RES0`.
pub const VMXNET3_TX_RES0: u32 = 0x00008000;
/// `VMXNET3_TX_DTYPE_M`: descriptor type.
pub const VMXNET3_TX_DTYPE_M: u32 = 0x00000001;
/// `VMXNET3_TX_DTYPE_S`: descriptor type.
pub const VMXNET3_TX_DTYPE_S: u32 = 16;
/// `VMXNET3_TX_RES1`.
pub const VMXNET3_TX_RES1: u32 = 0x00000002;
/// `VMXNET3_TX_OP_M`: offloading position.
pub const VMXNET3_TX_OP_M: u32 = 0x00003fff;
/// `VMXNET3_TX_OP_S`.
pub const VMXNET3_TX_OP_S: u32 = 18;

/// `VMXNET3_TX_HLEN_M`: header len.
pub const VMXNET3_TX_HLEN_M: u32 = 0x000003ff;
/// `VMXNET3_TX_HLEN_S`.
pub const VMXNET3_TX_HLEN_S: u32 = 0;
/// `VMXNET3_TX_OM_M`: offloading mode.
pub const VMXNET3_TX_OM_M: u32 = 0x00000003;
/// `VMXNET3_TX_OM_S`.
pub const VMXNET3_TX_OM_S: u32 = 10;
/// `VMXNET3_TX_EOP`: end of packet.
pub const VMXNET3_TX_EOP: u32 = 0x00001000;
/// `VMXNET3_TX_COMPREQ`: completion request.
pub const VMXNET3_TX_COMPREQ: u32 = 0x00002000;
/// `VMXNET3_TX_RES2`.
pub const VMXNET3_TX_RES2: u32 = 0x00004000;
/// `VMXNET3_TX_VTAG_MODE`: VLAN tag insertion mode.
pub const VMXNET3_TX_VTAG_MODE: u32 = 0x00008000;
/// `VMXNET3_TX_VLANTAG_M`.
pub const VMXNET3_TX_VLANTAG_M: u32 = 0x0000ffff;
/// `VMXNET3_TX_VLANTAG_S`.
pub const VMXNET3_TX_VLANTAG_S: u32 = 16;

// offloading modes

/// `VMXNET3_OM_NONE`.
pub const VMXNET3_OM_NONE: u32 = 0;
/// `VMXNET3_OM_CSUM`.
pub const VMXNET3_OM_CSUM: u32 = 2;
/// `VMXNET3_OM_TSO`.
pub const VMXNET3_OM_TSO: u32 = 3;

/// `struct vmxnet3_txcompdesc`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vmxnet3Txcompdesc {
    /// `txc_word0`.
    pub txc_word0: u32,
    /// `txc_word1`.
    pub txc_word1: u32,
    /// `txc_word2`.
    pub txc_word2: u32,
    /// `txc_word3`.
    pub txc_word3: u32,
}

/// `VMXNET3_TXC_EOPIDX_M`: eop index in Tx ring.
pub const VMXNET3_TXC_EOPIDX_M: u32 = 0x00000fff;
/// `VMXNET3_TXC_EOPIDX_S`.
pub const VMXNET3_TXC_EOPIDX_S: u32 = 0;
/// `VMXNET3_TXC_RES0_M`.
pub const VMXNET3_TXC_RES0_M: u32 = 0x000fffff;
/// `VMXNET3_TXC_RES0_S`.
pub const VMXNET3_TXC_RES0_S: u32 = 12;

/// `VMXNET3_TXC_RES2_M`.
pub const VMXNET3_TXC_RES2_M: u32 = 0x00ffffff;
/// `VMXNET3_TXC_TYPE_M`.
pub const VMXNET3_TXC_TYPE_M: u32 = 0x0000007f;
/// `VMXNET3_TXC_TYPE_S`.
pub const VMXNET3_TXC_TYPE_S: u32 = 24;
/// `VMXNET3_TXC_GEN_M`.
pub const VMXNET3_TXC_GEN_M: u32 = 0x00000001;
/// `VMXNET3_TXC_GEN_S`.
pub const VMXNET3_TXC_GEN_S: u32 = 31;

/// `struct vmxnet3_rxdesc`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vmxnet3Rxdesc {
    /// `rx_addr`.
    pub rx_addr: u64,
    /// `rx_word2`.
    pub rx_word2: u32,
    /// `rx_word3`.
    pub rx_word3: u32,
}

/// `VMXNET3_RX_LEN_M`.
pub const VMXNET3_RX_LEN_M: u32 = 0x00003fff;
/// `VMXNET3_RX_LEN_S`.
pub const VMXNET3_RX_LEN_S: u32 = 0;
/// `VMXNET3_RX_BTYPE_M`: buffer type.
pub const VMXNET3_RX_BTYPE_M: u32 = 0x00000001;
/// `VMXNET3_RX_BTYPE_S`.
pub const VMXNET3_RX_BTYPE_S: u32 = 14;
/// `VMXNET3_RX_DTYPE_M`: descriptor type.
pub const VMXNET3_RX_DTYPE_M: u32 = 0x00000001;
/// `VMXNET3_RX_DTYPE_S`.
pub const VMXNET3_RX_DTYPE_S: u32 = 15;
/// `VMXNET3_RX_RES0_M`.
pub const VMXNET3_RX_RES0_M: u32 = 0x00007fff;
/// `VMXNET3_RX_RES0_S`.
pub const VMXNET3_RX_RES0_S: u32 = 16;
/// `VMXNET3_RX_GEN_M`.
pub const VMXNET3_RX_GEN_M: u32 = 0x00000001;
/// `VMXNET3_RX_GEN_S`.
pub const VMXNET3_RX_GEN_S: u32 = 31;

// buffer types

/// `VMXNET3_BTYPE_HEAD`: head only.
pub const VMXNET3_BTYPE_HEAD: u32 = 0;
/// `VMXNET3_BTYPE_BODY`: body only.
pub const VMXNET3_BTYPE_BODY: u32 = 1;

/// `struct vmxnet3_rxcompdesc`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vmxnet3Rxcompdesc {
    /// `rxc_word0`.
    pub rxc_word0: u32,
    /// `rxc_word1`.
    pub rxc_word1: u32,
    /// `rxc_word2`.
    pub rxc_word2: u32,
    /// `rxc_word3`.
    pub rxc_word3: u32,
}

/// `VMXNET3_RXC_IDX_M`: Rx descriptor index.
pub const VMXNET3_RXC_IDX_M: u32 = 0x00000fff;
/// `VMXNET3_RXC_IDX_S`.
pub const VMXNET3_RXC_IDX_S: u32 = 0;
/// `VMXNET3_RXC_RES0_M`.
pub const VMXNET3_RXC_RES0_M: u32 = 0x00000003;
/// `VMXNET3_RXC_RES0_S`.
pub const VMXNET3_RXC_RES0_S: u32 = 12;
/// `VMXNET3_RXC_EOP`: end of packet.
pub const VMXNET3_RXC_EOP: u32 = 0x00004000;
/// `VMXNET3_RXC_SOP`: start of packet.
pub const VMXNET3_RXC_SOP: u32 = 0x00008000;
/// `VMXNET3_RXC_QID_M`.
pub const VMXNET3_RXC_QID_M: u32 = 0x000003ff;
/// `VMXNET3_RXC_QID_S`.
pub const VMXNET3_RXC_QID_S: u32 = 16;
/// `VMXNET3_RXC_RSSTYPE_M`.
pub const VMXNET3_RXC_RSSTYPE_M: u32 = 0x0000000f;
/// `VMXNET3_RXC_RSSTYPE_S`.
pub const VMXNET3_RXC_RSSTYPE_S: u32 = 26;
/// `VMXNET3_RXC_RSSTYPE_NONE`.
pub const VMXNET3_RXC_RSSTYPE_NONE: u32 = 0;
/// `VMXNET3_RXC_NOCSUM`: no checksum calculated.
pub const VMXNET3_RXC_NOCSUM: u32 = 0x40000000;
/// `VMXNET3_RXC_RES1`.
pub const VMXNET3_RXC_RES1: u32 = 0x80000000;

/// `VMXNET3_RXC_RSSHASH_M`: RSS hash value.
pub const VMXNET3_RXC_RSSHASH_M: u32 = 0xffffffff;
/// `VMXNET3_RXC_RSSHASH_S`.
pub const VMXNET3_RXC_RSSHASH_S: u32 = 0;
/// `VMXNET3_RXC_SEG_CNT_M`: No. of seg. in LRO pkt.
pub const VMXNET3_RXC_SEG_CNT_M: u32 = 0x000000ff;

/// `VMXNET3_RXC_LEN_M`.
pub const VMXNET3_RXC_LEN_M: u32 = 0x00003fff;
/// `VMXNET3_RXC_LEN_S`.
pub const VMXNET3_RXC_LEN_S: u32 = 0;
/// `VMXNET3_RXC_ERROR`.
pub const VMXNET3_RXC_ERROR: u32 = 0x00004000;
/// `VMXNET3_RXC_VLAN`: 802.1Q VLAN frame.
pub const VMXNET3_RXC_VLAN: u32 = 0x00008000;
/// `VMXNET3_RXC_VLANTAG_M`: VLAN tag.
pub const VMXNET3_RXC_VLANTAG_M: u32 = 0x0000ffff;
/// `VMXNET3_RXC_VLANTAG_S`.
pub const VMXNET3_RXC_VLANTAG_S: u32 = 16;

/// `VMXNET3_RXC_CSUM_M`: TCP/UDP checksum.
pub const VMXNET3_RXC_CSUM_M: u32 = 0x0000ffff;
/// `VMXNET3_RXC_CSUM_S`.
pub const VMXNET3_RXC_CSUM_S: u32 = 16;
/// `VMXNET3_RXC_CSUM_OK`: TCP/UDP checksum ok.
pub const VMXNET3_RXC_CSUM_OK: u32 = 0x00010000;
/// `VMXNET3_RXC_UDP`.
pub const VMXNET3_RXC_UDP: u32 = 0x00020000;
/// `VMXNET3_RXC_TCP`.
pub const VMXNET3_RXC_TCP: u32 = 0x00040000;
/// `VMXNET3_RXC_IPSUM_OK`: IP checksum ok.
pub const VMXNET3_RXC_IPSUM_OK: u32 = 0x00080000;
/// `VMXNET3_RXC_IPV6`.
pub const VMXNET3_RXC_IPV6: u32 = 0x00100000;
/// `VMXNET3_RXC_IPV4`.
pub const VMXNET3_RXC_IPV4: u32 = 0x00200000;
/// `VMXNET3_RXC_FRAGMENT`: IP fragment.
pub const VMXNET3_RXC_FRAGMENT: u32 = 0x00400000;
/// `VMXNET3_RXC_FCS`: frame CRC correct.
pub const VMXNET3_RXC_FCS: u32 = 0x00800000;
/// `VMXNET3_RXC_TYPE_M`.
pub const VMXNET3_RXC_TYPE_M: u32 = 0x7f000000;
/// `VMXNET3_RXC_TYPE_S`.
pub const VMXNET3_RXC_TYPE_S: u32 = 24;
/// `VMXNET3_RXC_GEN_M`.
pub const VMXNET3_RXC_GEN_M: u32 = 0x00000001;
/// `VMXNET3_RXC_GEN_S`.
pub const VMXNET3_RXC_GEN_S: u32 = 31;

/// `VMXNET3_REV1_MAGIC`.
pub const VMXNET3_REV1_MAGIC: u32 = 0xbabefee1;

/// `VMXNET3_GOS_UNKNOWN`.
pub const VMXNET3_GOS_UNKNOWN: u32 = 0x00;
/// `VMXNET3_GOS_LINUX`.
pub const VMXNET3_GOS_LINUX: u32 = 0x04;
/// `VMXNET3_GOS_WINDOWS`.
pub const VMXNET3_GOS_WINDOWS: u32 = 0x08;
/// `VMXNET3_GOS_SOLARIS`.
pub const VMXNET3_GOS_SOLARIS: u32 = 0x0c;
/// `VMXNET3_GOS_FREEBSD`.
pub const VMXNET3_GOS_FREEBSD: u32 = 0x10;
/// `VMXNET3_GOS_PXE`.
pub const VMXNET3_GOS_PXE: u32 = 0x14;

/// `VMXNET3_GOS_32BIT`.
pub const VMXNET3_GOS_32BIT: u32 = 0x01;
/// `VMXNET3_GOS_64BIT`.
pub const VMXNET3_GOS_64BIT: u32 = 0x02;

/// `VMXNET3_MAX_TX_QUEUES`.
pub const VMXNET3_MAX_TX_QUEUES: usize = 8;
/// `VMXNET3_MAX_RX_QUEUES`.
pub const VMXNET3_MAX_RX_QUEUES: usize = 16;
/// `VMXNET3_MAX_INTRS`.
pub const VMXNET3_MAX_INTRS: usize = VMXNET3_MAX_TX_QUEUES + VMXNET3_MAX_RX_QUEUES + 1;
/// `VMXNET3_NINTR`.
pub const VMXNET3_NINTR: u32 = 1;

/// `VMXNET3_ICTRL_DISABLE_ALL`.
pub const VMXNET3_ICTRL_DISABLE_ALL: u32 = 0x01;

/// `VMXNET3_RXMODE_UCAST`.
pub const VMXNET3_RXMODE_UCAST: u32 = 0x01;
/// `VMXNET3_RXMODE_MCAST`.
pub const VMXNET3_RXMODE_MCAST: u32 = 0x02;
/// `VMXNET3_RXMODE_BCAST`.
pub const VMXNET3_RXMODE_BCAST: u32 = 0x04;
/// `VMXNET3_RXMODE_ALLMULTI`.
pub const VMXNET3_RXMODE_ALLMULTI: u32 = 0x08;
/// `VMXNET3_RXMODE_PROMISC`.
pub const VMXNET3_RXMODE_PROMISC: u32 = 0x10;

/// `VMXNET3_EVENT_RQERROR`.
pub const VMXNET3_EVENT_RQERROR: u32 = 0x01;
/// `VMXNET3_EVENT_TQERROR`.
pub const VMXNET3_EVENT_TQERROR: u32 = 0x02;
/// `VMXNET3_EVENT_LINK`.
pub const VMXNET3_EVENT_LINK: u32 = 0x04;
/// `VMXNET3_EVENT_DIC`.
pub const VMXNET3_EVENT_DIC: u32 = 0x08;
/// `VMXNET3_EVENT_DEBUG`.
pub const VMXNET3_EVENT_DEBUG: u32 = 0x10;

/// `VMXNET3_MAX_MTU`.
pub const VMXNET3_MAX_MTU: u32 = 9000;
/// `VMXNET3_MIN_MTU`.
pub const VMXNET3_MIN_MTU: u32 = 60;

/// The anonymous `struct { version; len; paddr; }` of `rss`, `pm` and `plugin` in
/// `struct vmxnet3_driver_shared`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vmxnet3SharedConf {
    /// `version`.
    pub version: u32,
    /// `len`.
    pub len: u32,
    /// `paddr`.
    pub paddr: u64,
}

/// `struct vmxnet3_driver_shared`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vmxnet3DriverShared {
    /// `magic`.
    pub magic: u32,
    /// `pad1`.
    pub pad1: u32,

    /// `version`: driver version.
    pub version: u32,
    /// `guest`: guest OS.
    pub guest: u32,
    /// `vmxnet3_revision`: supported VMXNET3 revision.
    pub vmxnet3_revision: u32,
    /// `upt_version`: supported UPT version.
    pub upt_version: u32,
    /// `upt_features`.
    pub upt_features: u64,
    /// `driver_data`.
    pub driver_data: u64,
    /// `queue_shared`.
    pub queue_shared: u64,
    /// `driver_data_len`.
    pub driver_data_len: u32,
    /// `queue_shared_len`.
    pub queue_shared_len: u32,
    /// `mtu`.
    pub mtu: u32,
    /// `nrxsg_max`.
    pub nrxsg_max: u16,
    /// `ntxqueue`.
    pub ntxqueue: u8,
    /// `nrxqueue`.
    pub nrxqueue: u8,
    /// `reserved1`.
    pub reserved1: [u32; 4],

    // interrupt control
    /// `automask`.
    pub automask: u8,
    /// `nintr`.
    pub nintr: u8,
    /// `evintr`.
    pub evintr: u8,
    /// `modlevel`.
    pub modlevel: [u8; VMXNET3_MAX_INTRS],
    /// `ictrl`.
    pub ictrl: u32,
    /// `reserved2`.
    pub reserved2: [u32; 2],

    // receive filter parameters
    /// `rxmode`.
    pub rxmode: u32,
    /// `mcast_tablelen`.
    pub mcast_tablelen: u16,
    /// `pad2`.
    pub pad2: u16,
    /// `mcast_table`.
    pub mcast_table: u64,
    /// `vlan_filter`.
    pub vlan_filter: [u32; 4096 / 32],

    /// `rss`.
    pub rss: Vmxnet3SharedConf,
    /// `pm`.
    pub pm: Vmxnet3SharedConf,
    /// `plugin`.
    pub plugin: Vmxnet3SharedConf,

    /// `event`.
    pub event: u32,
    /// `reserved3`.
    pub reserved3: [u32; 5],
}

impl Vmxnet3DriverShared {
    /// The structure zeroed.
    pub const ZERO: Self = Self {
        magic: 0,
        pad1: 0,
        version: 0,
        guest: 0,
        vmxnet3_revision: 0,
        upt_version: 0,
        upt_features: 0,
        driver_data: 0,
        queue_shared: 0,
        driver_data_len: 0,
        queue_shared_len: 0,
        mtu: 0,
        nrxsg_max: 0,
        ntxqueue: 0,
        nrxqueue: 0,
        reserved1: [0; 4],
        automask: 0,
        nintr: 0,
        evintr: 0,
        modlevel: [0; VMXNET3_MAX_INTRS],
        ictrl: 0,
        reserved2: [0; 2],
        rxmode: 0,
        mcast_tablelen: 0,
        pad2: 0,
        mcast_table: 0,
        vlan_filter: [0; 4096 / 32],
        rss: Vmxnet3SharedConf {
            version: 0,
            len: 0,
            paddr: 0,
        },
        pm: Vmxnet3SharedConf {
            version: 0,
            len: 0,
            paddr: 0,
        },
        plugin: Vmxnet3SharedConf {
            version: 0,
            len: 0,
            paddr: 0,
        },
        event: 0,
        reserved3: [0; 5],
    };
}

/// `struct vmxnet3_txq_shared`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vmxnet3TxqShared {
    /// `npending`.
    pub npending: u32,
    /// `intr_threshold`.
    pub intr_threshold: u32,
    /// `reserved1`.
    pub reserved1: u64,

    /// `cmd_ring`.
    pub cmd_ring: u64,
    /// `data_ring`.
    pub data_ring: u64,
    /// `comp_ring`.
    pub comp_ring: u64,
    /// `driver_data`.
    pub driver_data: u64,
    /// `reserved2`.
    pub reserved2: u64,
    /// `cmd_ring_len`.
    pub cmd_ring_len: u32,
    /// `data_ring_len`.
    pub data_ring_len: u32,
    /// `comp_ring_len`.
    pub comp_ring_len: u32,
    /// `driver_data_len`.
    pub driver_data_len: u32,
    /// `intr_idx`.
    pub intr_idx: u8,
    /// `pad1`.
    pub pad1: [u8; 7],

    /// `stopped`.
    pub stopped: u8,
    /// `pad2`.
    pub pad2: [u8; 3],
    /// `error`.
    pub error: u32,

    /// `stats`.
    pub stats: [u64; UPT1_TxStats_count as usize],

    /// `pad3`.
    pub pad3: [u8; 88],
}

impl Vmxnet3TxqShared {
    /// The structure zeroed.
    pub const ZERO: Self = Self {
        npending: 0,
        intr_threshold: 0,
        reserved1: 0,
        cmd_ring: 0,
        data_ring: 0,
        comp_ring: 0,
        driver_data: 0,
        reserved2: 0,
        cmd_ring_len: 0,
        data_ring_len: 0,
        comp_ring_len: 0,
        driver_data_len: 0,
        intr_idx: 0,
        pad1: [0; 7],
        stopped: 0,
        pad2: [0; 3],
        error: 0,
        stats: [0; UPT1_TxStats_count as usize],
        pad3: [0; 88],
    };
}

/// `struct vmxnet3_rxq_shared`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vmxnet3RxqShared {
    /// `update_rxhead`.
    pub update_rxhead: u8,
    /// `pad1`.
    pub pad1: [u8; 7],
    /// `reserved1`.
    pub reserved1: u64,

    /// `cmd_ring`.
    pub cmd_ring: [u64; 2],
    /// `comp_ring`.
    pub comp_ring: u64,
    /// `driver_data`.
    pub driver_data: u64,
    /// `reserved2`.
    pub reserved2: u64,
    /// `cmd_ring_len`.
    pub cmd_ring_len: [u32; 2],
    /// `comp_ring_len`.
    pub comp_ring_len: u32,
    /// `driver_data_len`.
    pub driver_data_len: u32,
    /// `intr_idx`.
    pub intr_idx: u8,
    /// `pad2`.
    pub pad2: [u8; 7],

    /// `stopped`.
    pub stopped: u8,
    /// `pad3`.
    pub pad3: [u8; 3],
    /// `error`.
    pub error: u32,

    /// `stats`.
    pub stats: [u64; UPT1_RxStats_count as usize],

    /// `pad4`.
    pub pad4: [u8; 88],
}

impl Vmxnet3RxqShared {
    /// The structure zeroed.
    pub const ZERO: Self = Self {
        update_rxhead: 0,
        pad1: [0; 7],
        reserved1: 0,
        cmd_ring: [0; 2],
        comp_ring: 0,
        driver_data: 0,
        reserved2: 0,
        cmd_ring_len: [0; 2],
        comp_ring_len: 0,
        driver_data_len: 0,
        intr_idx: 0,
        pad2: [0; 7],
        stopped: 0,
        pad3: [0; 3],
        error: 0,
        stats: [0; UPT1_RxStats_count as usize],
        pad4: [0; 88],
    };
}

/// `UPT1_RSS_MAX_KEY_SIZE`.
pub const UPT1_RSS_MAX_KEY_SIZE: usize = 40;
/// `UPT1_RSS_MAX_IND_TABLE_SIZE`.
pub const UPT1_RSS_MAX_IND_TABLE_SIZE: usize = 128;

/// `struct vmxnet3_upt1_rss_conf`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vmxnet3Upt1RssConf {
    /// `hash_type`.
    pub hash_type: u16,
    /// `hash_func`.
    pub hash_func: u16,
    /// `hash_key_size`.
    pub hash_key_size: u16,
    /// `ind_table_size`.
    pub ind_table_size: u16,
    /// `hash_key`.
    pub hash_key: [u8; UPT1_RSS_MAX_KEY_SIZE],
    /// `ind_table`.
    pub ind_table: [u8; UPT1_RSS_MAX_IND_TABLE_SIZE],
}

/// `UPT1_RSS_HASH_TYPE_NONE`.
pub const UPT1_RSS_HASH_TYPE_NONE: u16 = 0;
/// `UPT1_RSS_HASH_TYPE_IPV4`.
pub const UPT1_RSS_HASH_TYPE_IPV4: u16 = 1;
/// `UPT1_RSS_HASH_TYPE_TCP_IPV4`.
pub const UPT1_RSS_HASH_TYPE_TCP_IPV4: u16 = 2;
/// `UPT1_RSS_HASH_TYPE_IPV6`.
pub const UPT1_RSS_HASH_TYPE_IPV6: u16 = 4;
/// `UPT1_RSS_HASH_TYPE_TCP_IPV6`.
pub const UPT1_RSS_HASH_TYPE_TCP_IPV6: u16 = 8;
/// `UPT1_RSS_HASH_FUNC_TOEPLITZ`.
pub const UPT1_RSS_HASH_FUNC_TOEPLITZ: u16 = 1;

impl Vmxnet3Upt1RssConf {
    /// The structure zeroed.
    pub const ZERO: Self = Self {
        hash_type: 0,
        hash_func: 0,
        hash_key_size: 0,
        ind_table_size: 0,
        hash_key: [0; UPT1_RSS_MAX_KEY_SIZE],
        ind_table: [0; UPT1_RSS_MAX_IND_TABLE_SIZE],
    };
}

// The C's packed layouts: every member is at its natural offset, so `#[repr(C)]` matches.
const _: () = {
    use core::mem::offset_of;

    assert!(size_of::<Vmxnet3Txdesc>() == 16);
    assert!(size_of::<Vmxnet3Txcompdesc>() == 16);
    assert!(size_of::<Vmxnet3Rxdesc>() == 16);
    assert!(size_of::<Vmxnet3Rxcompdesc>() == 16);

    assert!(size_of::<Vmxnet3DriverShared>() == 720);
    assert!(offset_of!(Vmxnet3DriverShared, upt_features) == 24);
    assert!(offset_of!(Vmxnet3DriverShared, mtu) == 56);
    assert!(offset_of!(Vmxnet3DriverShared, automask) == 80);
    assert!(offset_of!(Vmxnet3DriverShared, modlevel) == 83);
    assert!(offset_of!(Vmxnet3DriverShared, ictrl) == 108);
    assert!(offset_of!(Vmxnet3DriverShared, rxmode) == 120);
    assert!(offset_of!(Vmxnet3DriverShared, mcast_table) == 128);
    assert!(offset_of!(Vmxnet3DriverShared, vlan_filter) == 136);
    assert!(offset_of!(Vmxnet3DriverShared, rss) == 648);
    assert!(offset_of!(Vmxnet3DriverShared, event) == 696);

    assert!(size_of::<Vmxnet3TxqShared>() == 256);
    assert!(offset_of!(Vmxnet3TxqShared, cmd_ring) == 16);
    assert!(offset_of!(Vmxnet3TxqShared, cmd_ring_len) == 56);
    assert!(offset_of!(Vmxnet3TxqShared, intr_idx) == 72);
    assert!(offset_of!(Vmxnet3TxqShared, stopped) == 80);
    assert!(offset_of!(Vmxnet3TxqShared, error) == 84);
    assert!(offset_of!(Vmxnet3TxqShared, stats) == 88);

    assert!(size_of::<Vmxnet3RxqShared>() == 256);
    assert!(offset_of!(Vmxnet3RxqShared, cmd_ring) == 16);
    assert!(offset_of!(Vmxnet3RxqShared, cmd_ring_len) == 56);
    assert!(offset_of!(Vmxnet3RxqShared, intr_idx) == 72);
    assert!(offset_of!(Vmxnet3RxqShared, stopped) == 80);
    assert!(offset_of!(Vmxnet3RxqShared, error) == 84);
    assert!(offset_of!(Vmxnet3RxqShared, stats) == 88);

    assert!(size_of::<Vmxnet3Upt1RssConf>() == 176);
    assert!(offset_of!(Vmxnet3Upt1RssConf, hash_key) == 8);
    assert!(offset_of!(Vmxnet3Upt1RssConf, ind_table) == 48);
};

#[cfg(test)]
mod tests;
