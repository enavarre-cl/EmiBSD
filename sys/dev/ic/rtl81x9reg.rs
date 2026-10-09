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
 * Copyright (c) 1997, 1998
 *	Bill Paul <wpaul@ctr.columbia.edu>.  All rights reserved.
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
 *	This product includes software developed by Bill Paul.
 * 4. Neither the name of the author nor the names of any co-contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY Bill Paul AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL Bill Paul OR THE VOICES IN HIS HEAD
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 *
 * $FreeBSD: src/sys/pci/if_rlreg.h,v 1.14 1999/10/21 19:42:03 wpaul Exp $
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/ic/rtl81x9reg.h>`: the registers of Realtek's 8129/8139 Fast Ethernet and
//! 8139C+/8169/8168/8101 controllers, the C+ descriptor format and `struct rl_softc`, the
//! softc rl(4) and re(4) share.
//!
//! Upstream: sys/dev/ic/rtl81x9reg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Every register offset and register value is a `u32` (the C's are untyped macros);
//!   the `CSR_*` accessors take `u32` offsets and narrow the value to the access width.
//!   `rl_cfg0`..`rl_cfg5` and `rl_txstart` (register offsets) are `u32` too.
//! - The function-like macros are `const fn`s with lower-case names (`RL_TXTHRESH`,
//!   `RL_IM_RXTIME`, `RL_IM_TXTIME`, `RL_TCPPKT`, `RL_UDPPKT`, `RL_ADDR_LO`, `RL_ADDR_HI`,
//!   `RL_PKTSZ`, `RL_NEXT_TXQ`); those that take the softc are methods of `RlSoftc`
//!   (`RL_TX_LIST_SZ`, `RL_RX_LIST_SZ`, `RL_NEXT_TX_DESC`, `RL_NEXT_RX_DESC`,
//!   `RL_TXDESCSYNC`, `RL_RXDESCSYNC`, `RL_RX_DMAMEM_SZ`, `RL_TXPADOFF`, `RL_TXPADDADDR`,
//!   `RL_CUR_*`, `RL_LAST_*`, and the `CSR_*` accessors); `RL_INC(x)` is `rl_inc(&Cell)`.
//! - `__STRICT_ALIGNMENT` is `sys::param::STRICT_ALIGNMENT` (arm64 defines it), so
//!   `RE_ETHER_ALIGN` and `RE_RX_DESC_BUFLEN` are one `const` each.
//! - `CSR_WRITE_RAW_4` writes the four bytes with `bus_space_write_4` of their native-order
//!   word: the same bytes in the same order as `bus_space_write_raw_region_4`, which is not
//!   ported, on both (little-endian) architectures.
//! - The descriptors (`struct rl_desc`) are `#[repr(C)]` plain data: the rings are DMA memory
//!   reached through raw pointers, every descriptor read and written whole and volatile
//!   (`RlSoftc::rxd_get` and friends), as em(4) does. `struct re_stats` is `#[repr(C,
//!   align(8))]`: its members fall on their natural offsets, so it has the C's packed layout.
//! - `struct rl_softc` is made of `Cell`s and atomics (all-zero valid: it lives in a zeroed
//!   softc); the bus tags and maps are `Option`s. `rl_flags` and the transmit ring's
//!   producer and consumer indices are atomics: `re_start` runs on whatever CPU serves the
//!   send queue, `re_txeof` in the (MP-safe) interrupt, the ioctls and the tick under the
//!   kernel lock. `rl_kstat` is left out: `kstat(4)` is not configured (`NKSTAT` 0).
//! - The `rl_*` prototypes at the end (`rl_attach`, `rl_intr`, `rl_detach`,
//!   `rl_activate`) belong to `rtl81x9.c`, which is not ported (rl(4), deferred).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::dev::mii::miivar::MiiData;
use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BusAddr, BusDmaSegment, BusDmaTag, BusDmamap, BusSize, BusSpaceHandle, BusSpaceTag,
    bus_dmamap_sync, bus_space_read_1, bus_space_read_2, bus_space_read_4, bus_space_write_1,
    bus_space_write_2, bus_space_write_4,
};
use crate::net::if_::IfRxring;
use crate::netinet::if_ether::{
    Arpcom, ETHER_CRC_LEN, ETHER_HDR_LEN, ETHER_VLAN_ENCAP_LEN, ETHERMTU,
};
use crate::sys::device::{Device, Softc};
use crate::sys::mbuf::{MCLBYTES, Mbuf};
use crate::sys::param::STRICT_ALIGNMENT;
use crate::sys::task::Task;
use crate::sys::timeout::Timeout;

/// `RL_IDR0`: ID register 0 (station addr).
pub const RL_IDR0: u32 = 0x0000;
/// `RL_IDR1`: Must use 32-bit accesses (?).
pub const RL_IDR1: u32 = 0x0001;
/// `RL_IDR2`.
pub const RL_IDR2: u32 = 0x0002;
/// `RL_IDR3`.
pub const RL_IDR3: u32 = 0x0003;
/// `RL_IDR4`.
pub const RL_IDR4: u32 = 0x0004;
/// `RL_IDR5`.
pub const RL_IDR5: u32 = 0x0005;
// 0006-0007 reserved
/// `RL_MAR0`: Multicast hash table.
pub const RL_MAR0: u32 = 0x0008;
/// `RL_MAR1`.
pub const RL_MAR1: u32 = 0x0009;
/// `RL_MAR2`.
pub const RL_MAR2: u32 = 0x000A;
/// `RL_MAR3`.
pub const RL_MAR3: u32 = 0x000B;
/// `RL_MAR4`.
pub const RL_MAR4: u32 = 0x000C;
/// `RL_MAR5`.
pub const RL_MAR5: u32 = 0x000D;
/// `RL_MAR6`.
pub const RL_MAR6: u32 = 0x000E;
/// `RL_MAR7`.
pub const RL_MAR7: u32 = 0x000F;

/// `RL_TXSTAT0`: status of TX descriptor 0.
pub const RL_TXSTAT0: u32 = 0x0010;
/// `RL_TXSTAT1`: status of TX descriptor 1.
pub const RL_TXSTAT1: u32 = 0x0014;
/// `RL_TXSTAT2`: status of TX descriptor 2.
pub const RL_TXSTAT2: u32 = 0x0018;
/// `RL_TXSTAT3`: status of TX descriptor 3.
pub const RL_TXSTAT3: u32 = 0x001C;

/// `RL_TXADDR0`: address of TX descriptor 0.
pub const RL_TXADDR0: u32 = 0x0020;
/// `RL_TXADDR1`: address of TX descriptor 1.
pub const RL_TXADDR1: u32 = 0x0024;
/// `RL_TXADDR2`: address of TX descriptor 2.
pub const RL_TXADDR2: u32 = 0x0028;
/// `RL_TXADDR3`: address of TX descriptor 3.
pub const RL_TXADDR3: u32 = 0x002C;

/// `RL_RXADDR`: RX ring start address.
pub const RL_RXADDR: u32 = 0x0030;
/// `RL_RX_EARLY_BYTES`: RX early byte count.
pub const RL_RX_EARLY_BYTES: u32 = 0x0034;
/// `RL_RX_EARLY_STAT`: RX early status.
pub const RL_RX_EARLY_STAT: u32 = 0x0036;
/// `RL_COMMAND`: command register.
pub const RL_COMMAND: u32 = 0x0037;
/// `RL_CURRXADDR`: current address of packet read.
pub const RL_CURRXADDR: u32 = 0x0038;
/// `RL_CURRXBUF`: current RX buffer address.
pub const RL_CURRXBUF: u32 = 0x003A;
/// `RL_IMR`: interrupt mask register.
pub const RL_IMR: u32 = 0x003C;
/// `RL_ISR`: interrupt status register.
pub const RL_ISR: u32 = 0x003E;
/// `RL_TXCFG`: transmit config.
pub const RL_TXCFG: u32 = 0x0040;
/// `RL_RXCFG`: receive config.
pub const RL_RXCFG: u32 = 0x0044;
/// `RL_TIMERCNT`: timer count register.
pub const RL_TIMERCNT: u32 = 0x0048;
/// `RL_MISSEDPKT`: missed packet counter.
pub const RL_MISSEDPKT: u32 = 0x004C;
/// `RL_EECMD`: EEPROM command register.
pub const RL_EECMD: u32 = 0x0050;

// RTL8139/RTL8139C+ only
/// `RL_8139_CFG0`: config register #0.
pub const RL_8139_CFG0: u32 = 0x0051;
/// `RL_8139_CFG1`: config register #1.
pub const RL_8139_CFG1: u32 = 0x0052;
/// `RL_8139_CFG3`: config register #3.
pub const RL_8139_CFG3: u32 = 0x0059;
/// `RL_8139_CFG4`: config register #4.
pub const RL_8139_CFG4: u32 = 0x005A;
/// `RL_8139_CFG5`: config register #5.
pub const RL_8139_CFG5: u32 = 0x00D8;

/// `RL_CFG0`: config register #0.
pub const RL_CFG0: u32 = 0x0051;
/// `RL_CFG1`: config register #1.
pub const RL_CFG1: u32 = 0x0052;
/// `RL_CFG2`: config register #2.
pub const RL_CFG2: u32 = 0x0053;
/// `RL_CFG3`: config register #3.
pub const RL_CFG3: u32 = 0x0054;
/// `RL_CFG4`: config register #4.
pub const RL_CFG4: u32 = 0x0055;
/// `RL_CFG5`: config register #5.
pub const RL_CFG5: u32 = 0x0056;
// 0057 reserved
/// `RL_MEDIASTAT`: media status register (8139).
pub const RL_MEDIASTAT: u32 = 0x0058;
// 0059-005A reserved
/// `RL_MII`: 8129 chip only.
pub const RL_MII: u32 = 0x005A;
/// `RL_HALTCLK`.
pub const RL_HALTCLK: u32 = 0x005B;
/// `RL_MULTIINTR`: multiple interrupt.
pub const RL_MULTIINTR: u32 = 0x005C;
/// `RL_PCIREV`: PCI revision value.
pub const RL_PCIREV: u32 = 0x005E;
// 005F reserved
/// `RL_TXSTAT_ALL`: TX status of all descriptors.
pub const RL_TXSTAT_ALL: u32 = 0x0060;

/// `RL_CSIDR`.
pub const RL_CSIDR: u32 = 0x0064;
/// `RL_CSIAR`.
pub const RL_CSIAR: u32 = 0x0068;

// Direct PHY access registers only available on 8139
/// `RL_BMCR`: PHY basic mode control.
pub const RL_BMCR: u32 = 0x0062;
/// `RL_BMSR`: PHY basic mode status.
pub const RL_BMSR: u32 = 0x0064;
/// `RL_ANAR`: PHY autoneg advert.
pub const RL_ANAR: u32 = 0x0066;
/// `RL_LPAR`: PHY link partner ability.
pub const RL_LPAR: u32 = 0x0068;
/// `RL_ANER`: PHY autoneg expansion.
pub const RL_ANER: u32 = 0x006A;

/// `RL_DISCCNT`: disconnect counter.
pub const RL_DISCCNT: u32 = 0x006C;
/// `RL_FALSECAR`: false carrier counter.
pub const RL_FALSECAR: u32 = 0x006E;
/// `RL_NWAYTST`: NWAY test register.
pub const RL_NWAYTST: u32 = 0x0070;
/// `RL_RX_ER`: RX_ER counter.
pub const RL_RX_ER: u32 = 0x0072;
/// `RL_CSCFG`: CS configuration register.
pub const RL_CSCFG: u32 = 0x0074;

/// `RL_DUMPSTATS_LO`: counter dump command register.
pub const RL_DUMPSTATS_LO: u32 = 0x0010;
/// `RL_DUMPSTATS_HI`: counter dump command register.
pub const RL_DUMPSTATS_HI: u32 = 0x0014;
/// `RL_TXLIST_ADDR_LO`: 64 bits, 256 byte alignment.
pub const RL_TXLIST_ADDR_LO: u32 = 0x0020;
/// `RL_TXLIST_ADDR_HI`: 64 bits, 256 byte alignment.
pub const RL_TXLIST_ADDR_HI: u32 = 0x0024;
/// `RL_TXLIST_ADDR_HPRIO_LO`: 64 bits, 256 byte aligned.
pub const RL_TXLIST_ADDR_HPRIO_LO: u32 = 0x0028;
/// `RL_TXLIST_ADDR_HPRIO_HI`: 64 bits, 256 byte aligned.
pub const RL_TXLIST_ADDR_HPRIO_HI: u32 = 0x002C;
/// `RL_TIMERINT`: interrupt on timer expire.
pub const RL_TIMERINT: u32 = 0x0054;
/// `RL_TXSTART`: 8 bits.
pub const RL_TXSTART: u32 = 0x00D9;
/// `RL_CPLUS_CMD`: 16 bits.
pub const RL_CPLUS_CMD: u32 = 0x00E0;
/// `RL_RXLIST_ADDR_LO`: 64 bits, 256 byte alignment.
pub const RL_RXLIST_ADDR_LO: u32 = 0x00E4;
/// `RL_RXLIST_ADDR_HI`: 64 bits, 256 byte alignment.
pub const RL_RXLIST_ADDR_HI: u32 = 0x00E8;
/// `RL_EARLY_TX_THRESH`: 8 bits.
pub const RL_EARLY_TX_THRESH: u32 = 0x00EC;

/// `RL_GTXSTART`: 8 bits.
pub const RL_GTXSTART: u32 = 0x0038;
/// `RL_TIMERINT_8169`: different offset than 8139.
pub const RL_TIMERINT_8169: u32 = 0x0058;
/// `RL_PHYAR`.
pub const RL_PHYAR: u32 = 0x0060;
/// `RL_TBICSR`.
pub const RL_TBICSR: u32 = 0x0064;
/// `RL_TBI_ANAR`.
pub const RL_TBI_ANAR: u32 = 0x0068;
/// `RL_TBI_LPAR`.
pub const RL_TBI_LPAR: u32 = 0x006A;
/// `RL_GMEDIASTAT`: 8 bits.
pub const RL_GMEDIASTAT: u32 = 0x006C;
/// `RL_MACDBG`: 8 bits.
pub const RL_MACDBG: u32 = 0x006D;
/// `RL_GPIO`: 8 bits.
pub const RL_GPIO: u32 = 0x006E;
/// `RL_PMCH`: 8 bits.
pub const RL_PMCH: u32 = 0x006F;
/// `RL_LDPS`: Link Down Power Saving.
pub const RL_LDPS: u32 = 0x0082;
/// `RL_MAXRXPKTLEN`: 16 bits, chip multiplies by 8.
pub const RL_MAXRXPKTLEN: u32 = 0x00DA;
/// `RL_IM`.
pub const RL_IM: u32 = 0x00E2;
/// `RL_MISC`.
pub const RL_MISC: u32 = 0x00F0;

/// `RL_LEDSEL`.
pub const RL_LEDSEL: u32 = 0x0018;
/// `RL_LED_LINK`: link at any speed.
pub const RL_LED_LINK: u32 = 0x7;
/// `RL_LED_ACT`.
pub const RL_LED_ACT: u32 = 0x8;

/// `RL_TXCFG_CLRABRT`: retransmit aborted pkt.
pub const RL_TXCFG_CLRABRT: u32 = 0x00000001;
/// `RL_TXCFG_MAXDMA`: max DMA burst size.
pub const RL_TXCFG_MAXDMA: u32 = 0x00000700;
/// `RL_TXCFG_QUEUE_EMPTY`: 8168E-VL or higher.
pub const RL_TXCFG_QUEUE_EMPTY: u32 = 0x00000800;
/// `RL_TXCFG_CRCAPPEND`: CRC append (0 = yes).
pub const RL_TXCFG_CRCAPPEND: u32 = 0x00010000;
/// `RL_TXCFG_LOOPBKTST`: loopback test.
pub const RL_TXCFG_LOOPBKTST: u32 = 0x00060000;
/// `RL_TXCFG_IFG2`: 8169 only.
pub const RL_TXCFG_IFG2: u32 = 0x00080000;
/// `RL_TXCFG_IFG`: interframe gap.
pub const RL_TXCFG_IFG: u32 = 0x03000000;
/// `RL_TXCFG_HWREV`.
pub const RL_TXCFG_HWREV: u32 = 0x7C800000;

/// `RL_LOOPTEST_OFF`.
pub const RL_LOOPTEST_OFF: u32 = 0x00000000;
/// `RL_LOOPTEST_ON`.
pub const RL_LOOPTEST_ON: u32 = 0x00020000;
/// `RL_LOOPTEST_ON_CPLUS`.
pub const RL_LOOPTEST_ON_CPLUS: u32 = 0x00060000;

// Known revision codes.

/// `RL_HWREV_8169`.
pub const RL_HWREV_8169: u32 = 0x00000000;
/// `RL_HWREV_8169S`.
pub const RL_HWREV_8169S: u32 = 0x00800000;
/// `RL_HWREV_8110S`.
pub const RL_HWREV_8110S: u32 = 0x04000000;
/// `RL_HWREV_8169_8110SB`.
pub const RL_HWREV_8169_8110SB: u32 = 0x10000000;
/// `RL_HWREV_8169_8110SCd`.
#[allow(non_upper_case_globals)] // the C's name, verbatim
pub const RL_HWREV_8169_8110SCd: u32 = 0x18000000;
/// `RL_HWREV_8401E`.
pub const RL_HWREV_8401E: u32 = 0x24000000;
/// `RL_HWREV_8102EL`.
pub const RL_HWREV_8102EL: u32 = 0x24800000;
/// `RL_HWREV_8102EL_SPIN1`.
pub const RL_HWREV_8102EL_SPIN1: u32 = 0x24C00000;
/// `RL_HWREV_8168D`.
pub const RL_HWREV_8168D: u32 = 0x28000000;
/// `RL_HWREV_8168DP`.
pub const RL_HWREV_8168DP: u32 = 0x28800000;
/// `RL_HWREV_8168E`.
pub const RL_HWREV_8168E: u32 = 0x2C000000;
/// `RL_HWREV_8168E_VL`.
pub const RL_HWREV_8168E_VL: u32 = 0x2C800000;
/// `RL_HWREV_8168B_SPIN1`.
pub const RL_HWREV_8168B_SPIN1: u32 = 0x30000000;
/// `RL_HWREV_8100E`.
pub const RL_HWREV_8100E: u32 = 0x30800000;
/// `RL_HWREV_8101E`.
pub const RL_HWREV_8101E: u32 = 0x34000000;
/// `RL_HWREV_8102E`.
pub const RL_HWREV_8102E: u32 = 0x34800000;
/// `RL_HWREV_8103E`.
pub const RL_HWREV_8103E: u32 = 0x34C00000;
/// `RL_HWREV_8168B_SPIN2`.
pub const RL_HWREV_8168B_SPIN2: u32 = 0x38000000;
/// `RL_HWREV_8168B_SPIN3`.
pub const RL_HWREV_8168B_SPIN3: u32 = 0x38400000;
/// `RL_HWREV_8100E_SPIN2`.
pub const RL_HWREV_8100E_SPIN2: u32 = 0x38800000;
/// `RL_HWREV_8168C`.
pub const RL_HWREV_8168C: u32 = 0x3c000000;
/// `RL_HWREV_8168C_SPIN2`.
pub const RL_HWREV_8168C_SPIN2: u32 = 0x3c400000;
/// `RL_HWREV_8168CP`.
pub const RL_HWREV_8168CP: u32 = 0x3c800000;
/// `RL_HWREV_8105E`.
pub const RL_HWREV_8105E: u32 = 0x40800000;
/// `RL_HWREV_8105E_SPIN1`.
pub const RL_HWREV_8105E_SPIN1: u32 = 0x40C00000;
/// `RL_HWREV_8402`.
pub const RL_HWREV_8402: u32 = 0x44000000;
/// `RL_HWREV_8106E`.
pub const RL_HWREV_8106E: u32 = 0x44800000;
/// `RL_HWREV_8168F`.
pub const RL_HWREV_8168F: u32 = 0x48000000;
/// `RL_HWREV_8411`.
pub const RL_HWREV_8411: u32 = 0x48800000;
/// `RL_HWREV_8168G`.
pub const RL_HWREV_8168G: u32 = 0x4c000000;
/// `RL_HWREV_8168EP`.
pub const RL_HWREV_8168EP: u32 = 0x50000000;
/// `RL_HWREV_8168GU`.
pub const RL_HWREV_8168GU: u32 = 0x50800000;
/// `RL_HWREV_8168H`.
pub const RL_HWREV_8168H: u32 = 0x54000000;
/// `RL_HWREV_8168FP`.
pub const RL_HWREV_8168FP: u32 = 0x54800000;
/// `RL_HWREV_8411B`.
pub const RL_HWREV_8411B: u32 = 0x5c800000;
/// `RL_HWREV_8139`.
pub const RL_HWREV_8139: u32 = 0x60000000;
/// `RL_HWREV_8139A`.
pub const RL_HWREV_8139A: u32 = 0x70000000;
/// `RL_HWREV_8139AG`.
pub const RL_HWREV_8139AG: u32 = 0x70800000;
/// `RL_HWREV_8139B`.
pub const RL_HWREV_8139B: u32 = 0x78000000;
/// `RL_HWREV_8130`.
pub const RL_HWREV_8130: u32 = 0x7C000000;
/// `RL_HWREV_8139C`.
pub const RL_HWREV_8139C: u32 = 0x74000000;
/// `RL_HWREV_8139D`.
pub const RL_HWREV_8139D: u32 = 0x74400000;
/// `RL_HWREV_8139CPLUS`.
pub const RL_HWREV_8139CPLUS: u32 = 0x74800000;
/// `RL_HWREV_8101`.
pub const RL_HWREV_8101: u32 = 0x74c00000;
/// `RL_HWREV_8100`.
pub const RL_HWREV_8100: u32 = 0x78800000;
/// `RL_HWREV_8169_8110SBL`.
pub const RL_HWREV_8169_8110SBL: u32 = 0x7cc00000;
/// `RL_HWREV_8169_8110SCe`.
#[allow(non_upper_case_globals)] // the C's name, verbatim
pub const RL_HWREV_8169_8110SCe: u32 = 0x98000000;

/// `RL_TXDMA_16BYTES`.
pub const RL_TXDMA_16BYTES: u32 = 0x00000000;
/// `RL_TXDMA_32BYTES`.
pub const RL_TXDMA_32BYTES: u32 = 0x00000100;
/// `RL_TXDMA_64BYTES`.
pub const RL_TXDMA_64BYTES: u32 = 0x00000200;
/// `RL_TXDMA_128BYTES`.
pub const RL_TXDMA_128BYTES: u32 = 0x00000300;
/// `RL_TXDMA_256BYTES`.
pub const RL_TXDMA_256BYTES: u32 = 0x00000400;
/// `RL_TXDMA_512BYTES`.
pub const RL_TXDMA_512BYTES: u32 = 0x00000500;
/// `RL_TXDMA_1024BYTES`.
pub const RL_TXDMA_1024BYTES: u32 = 0x00000600;
/// `RL_TXDMA_2048BYTES`.
pub const RL_TXDMA_2048BYTES: u32 = 0x00000700;

/// `RL_TXSTAT_LENMASK`.
pub const RL_TXSTAT_LENMASK: u32 = 0x00001FFF;
/// `RL_TXSTAT_OWN`.
pub const RL_TXSTAT_OWN: u32 = 0x00002000;
/// `RL_TXSTAT_TX_UNDERRUN`.
pub const RL_TXSTAT_TX_UNDERRUN: u32 = 0x00004000;
/// `RL_TXSTAT_TX_OK`.
pub const RL_TXSTAT_TX_OK: u32 = 0x00008000;
/// `RL_TXSTAT_EARLY_THRESH`.
pub const RL_TXSTAT_EARLY_THRESH: u32 = 0x003F0000;
/// `RL_TXSTAT_COLLCNT`.
pub const RL_TXSTAT_COLLCNT: u32 = 0x0F000000;
/// `RL_TXSTAT_CARR_HBEAT`.
pub const RL_TXSTAT_CARR_HBEAT: u32 = 0x10000000;
/// `RL_TXSTAT_OUTOFWIN`.
pub const RL_TXSTAT_OUTOFWIN: u32 = 0x20000000;
/// `RL_TXSTAT_TXABRT`.
pub const RL_TXSTAT_TXABRT: u32 = 0x40000000;
/// `RL_TXSTAT_CARRLOSS`.
pub const RL_TXSTAT_CARRLOSS: u32 = 0x80000000;

/// `RL_ISR_RX_OK`.
pub const RL_ISR_RX_OK: u32 = 0x0001;
/// `RL_ISR_RX_ERR`.
pub const RL_ISR_RX_ERR: u32 = 0x0002;
/// `RL_ISR_TX_OK`.
pub const RL_ISR_TX_OK: u32 = 0x0004;
/// `RL_ISR_TX_ERR`.
pub const RL_ISR_TX_ERR: u32 = 0x0008;
/// `RL_ISR_RX_OVERRUN`.
pub const RL_ISR_RX_OVERRUN: u32 = 0x0010;
/// `RL_ISR_PKT_UNDERRUN`.
pub const RL_ISR_PKT_UNDERRUN: u32 = 0x0020;
/// `RL_ISR_LINKCHG`: 8169 only.
pub const RL_ISR_LINKCHG: u32 = 0x0020;
/// `RL_ISR_FIFO_OFLOW`.
pub const RL_ISR_FIFO_OFLOW: u32 = 0x0040;
/// `RL_ISR_TX_DESC_UNAVAIL`: C+ only.
pub const RL_ISR_TX_DESC_UNAVAIL: u32 = 0x0080;
/// `RL_ISR_SWI`: C+ only.
pub const RL_ISR_SWI: u32 = 0x0100;
/// `RL_ISR_CABLE_LEN_CHGD`.
pub const RL_ISR_CABLE_LEN_CHGD: u32 = 0x2000;
/// `RL_ISR_PCS_TIMEOUT`: 8129 only.
pub const RL_ISR_PCS_TIMEOUT: u32 = 0x4000;
/// `RL_ISR_TIMEOUT_EXPIRED`.
pub const RL_ISR_TIMEOUT_EXPIRED: u32 = 0x4000;
/// `RL_ISR_SYSTEM_ERR`.
pub const RL_ISR_SYSTEM_ERR: u32 = 0x8000;

/// `RL_INTRS`.
pub const RL_INTRS: u32 = RL_ISR_TX_OK
    | RL_ISR_RX_OK
    | RL_ISR_RX_ERR
    | RL_ISR_TX_ERR
    | RL_ISR_RX_OVERRUN
    | RL_ISR_PKT_UNDERRUN
    | RL_ISR_FIFO_OFLOW
    | RL_ISR_PCS_TIMEOUT
    | RL_ISR_SYSTEM_ERR;

/// `RL_INTRS_CPLUS`.
pub const RL_INTRS_CPLUS: u32 = RL_ISR_RX_OK
    | RL_ISR_RX_ERR
    | RL_ISR_TX_ERR
    | RL_ISR_RX_OVERRUN
    | RL_ISR_FIFO_OFLOW
    | RL_ISR_SYSTEM_ERR
    | RL_ISR_TX_OK;

/// `RL_INTRS_TIMER`.
pub const RL_INTRS_TIMER: u32 =
    RL_ISR_RX_ERR | RL_ISR_TX_ERR | RL_ISR_SYSTEM_ERR | RL_ISR_TIMEOUT_EXPIRED;

/// `RL_MEDIASTAT_RXPAUSE`.
pub const RL_MEDIASTAT_RXPAUSE: u32 = 0x01;
/// `RL_MEDIASTAT_TXPAUSE`.
pub const RL_MEDIASTAT_TXPAUSE: u32 = 0x02;
/// `RL_MEDIASTAT_LINK`.
pub const RL_MEDIASTAT_LINK: u32 = 0x04;
/// `RL_MEDIASTAT_SPEED10`.
pub const RL_MEDIASTAT_SPEED10: u32 = 0x08;
/// `RL_MEDIASTAT_RXFLOWCTL`: duplex mode.
pub const RL_MEDIASTAT_RXFLOWCTL: u32 = 0x40;
/// `RL_MEDIASTAT_TXFLOWCTL`: duplex mode.
pub const RL_MEDIASTAT_TXFLOWCTL: u32 = 0x80;

/// `RL_RXCFG_RX_ALLPHYS`: accept all nodes.
pub const RL_RXCFG_RX_ALLPHYS: u32 = 0x00000001;
/// `RL_RXCFG_RX_INDIV`: match filter.
pub const RL_RXCFG_RX_INDIV: u32 = 0x00000002;
/// `RL_RXCFG_RX_MULTI`: accept all multicast.
pub const RL_RXCFG_RX_MULTI: u32 = 0x00000004;
/// `RL_RXCFG_RX_BROAD`: accept all broadcast.
pub const RL_RXCFG_RX_BROAD: u32 = 0x00000008;
/// `RL_RXCFG_RX_RUNT`.
pub const RL_RXCFG_RX_RUNT: u32 = 0x00000010;
/// `RL_RXCFG_RX_ERRPKT`.
pub const RL_RXCFG_RX_ERRPKT: u32 = 0x00000020;
/// `RL_RXCFG_WRAP`.
pub const RL_RXCFG_WRAP: u32 = 0x00000080;
/// `RL_RXCFG_EARLYOFFV2`.
pub const RL_RXCFG_EARLYOFFV2: u32 = 0x00000800;
/// `RL_RXCFG_MAXDMA`.
pub const RL_RXCFG_MAXDMA: u32 = 0x00000700;
/// `RL_RXCFG_BURSZ`.
pub const RL_RXCFG_BURSZ: u32 = 0x00001800;
/// `RL_RXCFG_EARLYOFF`.
pub const RL_RXCFG_EARLYOFF: u32 = 0x00003800;
/// `RL_RXCFG_FIFOTHRESH`.
pub const RL_RXCFG_FIFOTHRESH: u32 = 0x0000E000;
/// `RL_RXCFG_EARLYTHRESH`.
pub const RL_RXCFG_EARLYTHRESH: u32 = 0x07000000;

/// `RL_RXDMA_16BYTES`.
pub const RL_RXDMA_16BYTES: u32 = 0x00000000;
/// `RL_RXDMA_32BYTES`.
pub const RL_RXDMA_32BYTES: u32 = 0x00000100;
/// `RL_RXDMA_64BYTES`.
pub const RL_RXDMA_64BYTES: u32 = 0x00000200;
/// `RL_RXDMA_128BYTES`.
pub const RL_RXDMA_128BYTES: u32 = 0x00000300;
/// `RL_RXDMA_256BYTES`.
pub const RL_RXDMA_256BYTES: u32 = 0x00000400;
/// `RL_RXDMA_512BYTES`.
pub const RL_RXDMA_512BYTES: u32 = 0x00000500;
/// `RL_RXDMA_1024BYTES`.
pub const RL_RXDMA_1024BYTES: u32 = 0x00000600;
/// `RL_RXDMA_UNLIMITED`.
pub const RL_RXDMA_UNLIMITED: u32 = 0x00000700;

/// `RL_RXBUF_8`.
pub const RL_RXBUF_8: u32 = 0x00000000;
/// `RL_RXBUF_16`.
pub const RL_RXBUF_16: u32 = 0x00000800;
/// `RL_RXBUF_32`.
pub const RL_RXBUF_32: u32 = 0x00001000;
/// `RL_RXBUF_64`.
pub const RL_RXBUF_64: u32 = 0x00001800;

/// `RL_RXFIFO_16BYTES`.
pub const RL_RXFIFO_16BYTES: u32 = 0x00000000;
/// `RL_RXFIFO_32BYTES`.
pub const RL_RXFIFO_32BYTES: u32 = 0x00002000;
/// `RL_RXFIFO_64BYTES`.
pub const RL_RXFIFO_64BYTES: u32 = 0x00004000;
/// `RL_RXFIFO_128BYTES`.
pub const RL_RXFIFO_128BYTES: u32 = 0x00006000;
/// `RL_RXFIFO_256BYTES`.
pub const RL_RXFIFO_256BYTES: u32 = 0x00008000;
/// `RL_RXFIFO_512BYTES`.
pub const RL_RXFIFO_512BYTES: u32 = 0x0000A000;
/// `RL_RXFIFO_1024BYTES`.
pub const RL_RXFIFO_1024BYTES: u32 = 0x0000C000;
/// `RL_RXFIFO_NOTHRESH`.
pub const RL_RXFIFO_NOTHRESH: u32 = 0x0000E000;

/// `RL_RXSTAT_RXOK`.
pub const RL_RXSTAT_RXOK: u32 = 0x00000001;
/// `RL_RXSTAT_ALIGNERR`.
pub const RL_RXSTAT_ALIGNERR: u32 = 0x00000002;
/// `RL_RXSTAT_CRCERR`.
pub const RL_RXSTAT_CRCERR: u32 = 0x00000004;
/// `RL_RXSTAT_GIANT`.
pub const RL_RXSTAT_GIANT: u32 = 0x00000008;
/// `RL_RXSTAT_RUNT`.
pub const RL_RXSTAT_RUNT: u32 = 0x00000010;
/// `RL_RXSTAT_BADSYM`.
pub const RL_RXSTAT_BADSYM: u32 = 0x00000020;
/// `RL_RXSTAT_BROAD`.
pub const RL_RXSTAT_BROAD: u32 = 0x00002000;
/// `RL_RXSTAT_INDIV`.
pub const RL_RXSTAT_INDIV: u32 = 0x00004000;
/// `RL_RXSTAT_MULTI`.
pub const RL_RXSTAT_MULTI: u32 = 0x00008000;
/// `RL_RXSTAT_LENMASK`.
pub const RL_RXSTAT_LENMASK: u32 = 0xFFFF0000;

/// `RL_RXSTAT_UNFINISHED`: DMA still in progress.
pub const RL_RXSTAT_UNFINISHED: u32 = 0xFFF0;
/// `RL_CMD_EMPTY_RXBUF`.
pub const RL_CMD_EMPTY_RXBUF: u32 = 0x0001;
/// `RL_CMD_TX_ENB`.
pub const RL_CMD_TX_ENB: u32 = 0x0004;
/// `RL_CMD_RX_ENB`.
pub const RL_CMD_RX_ENB: u32 = 0x0008;
/// `RL_CMD_RESET`.
pub const RL_CMD_RESET: u32 = 0x0010;
/// `RL_CMD_STOPREQ`.
pub const RL_CMD_STOPREQ: u32 = 0x0080;

/// `RL_EE_DATAOUT`: Data out.
pub const RL_EE_DATAOUT: u32 = 0x01;
/// `RL_EE_DATAIN`: Data in.
pub const RL_EE_DATAIN: u32 = 0x02;
/// `RL_EE_CLK`: clock.
pub const RL_EE_CLK: u32 = 0x04;
/// `RL_EE_SEL`: chip select.
pub const RL_EE_SEL: u32 = 0x08;
/// `RL_EE_MODE`.
pub const RL_EE_MODE: u32 = 0x40 | 0x80;

/// `RL_EEMODE_OFF`.
pub const RL_EEMODE_OFF: u32 = 0x00;
/// `RL_EEMODE_AUTOLOAD`.
pub const RL_EEMODE_AUTOLOAD: u32 = 0x40;
/// `RL_EEMODE_PROGRAM`.
pub const RL_EEMODE_PROGRAM: u32 = 0x80;
/// `RL_EEMODE_WRITECFG`.
pub const RL_EEMODE_WRITECFG: u32 = 0x80 | 0x40;

// 9346/9356 EEPROM commands

/// `RL_9346_ADDR_LEN`: 93C46 1K: 128x16.
pub const RL_9346_ADDR_LEN: u32 = 6;
/// `RL_9356_ADDR_LEN`: 93C56 2K: 256x16.
pub const RL_9356_ADDR_LEN: u32 = 8;

/// `RL_9346_WRITE`.
pub const RL_9346_WRITE: u32 = 0x5;
/// `RL_9346_READ`.
pub const RL_9346_READ: u32 = 0x6;
/// `RL_9346_ERASE`.
pub const RL_9346_ERASE: u32 = 0x7;
/// `RL_9346_EWEN`.
pub const RL_9346_EWEN: u32 = 0x4;
/// `RL_9346_EWEN_ADDR`.
pub const RL_9346_EWEN_ADDR: u32 = 0x30;
/// `RL_9456_EWDS`.
pub const RL_9456_EWDS: u32 = 0x4;
/// `RL_9346_EWDS_ADDR`.
pub const RL_9346_EWDS_ADDR: u32 = 0x00;

/// `RL_EECMD_WRITE`: 0101b.
pub const RL_EECMD_WRITE: u32 = 0x5;
/// `RL_EECMD_READ`: 0110b.
pub const RL_EECMD_READ: u32 = 0x6;
/// `RL_EECMD_ERASE`: 0111b.
pub const RL_EECMD_ERASE: u32 = 0x7;
/// `RL_EECMD_LEN`.
pub const RL_EECMD_LEN: u32 = 4;

/// `RL_EEADDR_LEN0`: 9346.
pub const RL_EEADDR_LEN0: u32 = 6;
/// `RL_EEADDR_LEN1`: 9356.
pub const RL_EEADDR_LEN1: u32 = 8;

/// `RL_EECMD_READ_6BIT`: XXX.
pub const RL_EECMD_READ_6BIT: u32 = 0x180;
/// `RL_EECMD_READ_8BIT`: EECMD_READ above maybe wrong?.
pub const RL_EECMD_READ_8BIT: u32 = 0x600;

/// `RL_EE_ID`.
pub const RL_EE_ID: u32 = 0x00;
/// `RL_EE_PCI_VID`.
pub const RL_EE_PCI_VID: u32 = 0x01;
/// `RL_EE_PCI_DID`.
pub const RL_EE_PCI_DID: u32 = 0x02;
// Location of station address inside EEPROM
/// `RL_EE_EADDR`.
pub const RL_EE_EADDR: u32 = 0x07;

/// `RL_MII_CLK`.
pub const RL_MII_CLK: u32 = 0x01;
/// `RL_MII_DATAIN`.
pub const RL_MII_DATAIN: u32 = 0x02;
/// `RL_MII_DATAOUT`.
pub const RL_MII_DATAOUT: u32 = 0x04;
/// `RL_MII_DIR`: 0 == input, 1 == output.
pub const RL_MII_DIR: u32 = 0x80;

/// `RL_CFG0_ROM0`.
pub const RL_CFG0_ROM0: u32 = 0x01;
/// `RL_CFG0_ROM1`.
pub const RL_CFG0_ROM1: u32 = 0x02;
/// `RL_CFG0_ROM2`.
pub const RL_CFG0_ROM2: u32 = 0x04;
/// `RL_CFG0_PL0`.
pub const RL_CFG0_PL0: u32 = 0x08;
/// `RL_CFG0_PL1`.
pub const RL_CFG0_PL1: u32 = 0x10;
/// `RL_CFG0_10MBPS`: 10 Mbps internal mode.
pub const RL_CFG0_10MBPS: u32 = 0x20;
/// `RL_CFG0_PCS`.
pub const RL_CFG0_PCS: u32 = 0x40;
/// `RL_CFG0_SCR`.
pub const RL_CFG0_SCR: u32 = 0x80;

/// `RL_CFG1_PWRDWN`.
pub const RL_CFG1_PWRDWN: u32 = 0x01;
/// `RL_CFG1_PME`.
pub const RL_CFG1_PME: u32 = 0x01;
/// `RL_CFG1_SLEEP`.
pub const RL_CFG1_SLEEP: u32 = 0x02;
/// `RL_CFG1_VPDEN`.
pub const RL_CFG1_VPDEN: u32 = 0x02;
/// `RL_CFG1_IOMAP`.
pub const RL_CFG1_IOMAP: u32 = 0x04;
/// `RL_CFG1_MEMMAP`.
pub const RL_CFG1_MEMMAP: u32 = 0x08;
/// `RL_CFG1_RSVD`.
pub const RL_CFG1_RSVD: u32 = 0x10;
/// `RL_CFG1_LWACT`.
pub const RL_CFG1_LWACT: u32 = 0x10;
/// `RL_CFG1_DRVLOAD`.
pub const RL_CFG1_DRVLOAD: u32 = 0x20;
/// `RL_CFG1_LED0`.
pub const RL_CFG1_LED0: u32 = 0x40;
/// `RL_CFG1_FULLDUPLEX`: 8129 only.
pub const RL_CFG1_FULLDUPLEX: u32 = 0x40;
/// `RL_CFG1_LED1`.
pub const RL_CFG1_LED1: u32 = 0x80;

/// `RL_CFG2_PCI_MASK`.
pub const RL_CFG2_PCI_MASK: u32 = 0x07;
/// `RL_CFG2_PCI_33MHZ`.
pub const RL_CFG2_PCI_33MHZ: u32 = 0x00;
/// `RL_CFG2_PCI_66MHZ`.
pub const RL_CFG2_PCI_66MHZ: u32 = 0x01;
/// `RL_CFG2_PCI_64BIT`.
pub const RL_CFG2_PCI_64BIT: u32 = 0x08;
/// `RL_CFG2_AUXPWR`.
pub const RL_CFG2_AUXPWR: u32 = 0x10;
/// `RL_CFG2_MSI`.
pub const RL_CFG2_MSI: u32 = 0x20;

/// `RL_CFG3_GRANTSEL`.
pub const RL_CFG3_GRANTSEL: u32 = 0x80;
/// `RL_CFG3_WOL_MAGIC`.
pub const RL_CFG3_WOL_MAGIC: u32 = 0x20;
/// `RL_CFG3_WOL_LINK`.
pub const RL_CFG3_WOL_LINK: u32 = 0x10;
/// `RL_CFG3_JUMBO_EN0`.
pub const RL_CFG3_JUMBO_EN0: u32 = 0x04;
/// `RL_CFG3_FAST_B2B`.
pub const RL_CFG3_FAST_B2B: u32 = 0x01;

/// `RL_CFG4_CUSTOM_LED`.
pub const RL_CFG4_CUSTOM_LED: u32 = 0x40;
/// `RL_CFG4_LWPTN`.
pub const RL_CFG4_LWPTN: u32 = 0x04;
/// `RL_CFG4_LWPME`.
pub const RL_CFG4_LWPME: u32 = 0x10;
/// `RL_CFG4_JUMBO_EN1`.
pub const RL_CFG4_JUMBO_EN1: u32 = 0x02;
/// `RL_CFG4_8168E_JUMBO_EN1`.
pub const RL_CFG4_8168E_JUMBO_EN1: u32 = 0x01;

/// `RL_CFG5_WOL_BCAST`.
pub const RL_CFG5_WOL_BCAST: u32 = 0x40;
/// `RL_CFG5_WOL_MCAST`.
pub const RL_CFG5_WOL_MCAST: u32 = 0x20;
/// `RL_CFG5_WOL_UCAST`.
pub const RL_CFG5_WOL_UCAST: u32 = 0x10;
/// `RL_CFG5_WOL_LANWAKE`.
pub const RL_CFG5_WOL_LANWAKE: u32 = 0x02;
/// `RL_CFG5_PME_STS`.
pub const RL_CFG5_PME_STS: u32 = 0x01;

// RL_DUMPSTATS_LO register

/// `RL_DUMPSTATS_START`.
pub const RL_DUMPSTATS_START: u32 = 0x00000008;

// Transmit start register

/// `RL_TXSTART_SWI`: generate TX interrupt.
pub const RL_TXSTART_SWI: u32 = 0x01;
/// `RL_TXSTART_START`: start normal queue transmit.
pub const RL_TXSTART_START: u32 = 0x40;
/// `RL_TXSTART_HPRIO_START`: start hi prio queue transmit.
pub const RL_TXSTART_HPRIO_START: u32 = 0x80;

/// `RL_CFG2_BUSFREQ`.
pub const RL_CFG2_BUSFREQ: u32 = 0x07;
/// `RL_CFG2_BUSWIDTH`.
pub const RL_CFG2_BUSWIDTH: u32 = 0x08;
/// `RL_CFG2_AUXPWRSTS`.
pub const RL_CFG2_AUXPWRSTS: u32 = 0x10;

/// `RL_BUSFREQ_33MHZ`.
pub const RL_BUSFREQ_33MHZ: u32 = 0x00;
/// `RL_BUSFREQ_66MHZ`.
pub const RL_BUSFREQ_66MHZ: u32 = 0x01;

/// `RL_BUSWIDTH_32BITS`.
pub const RL_BUSWIDTH_32BITS: u32 = 0x00;
/// `RL_BUSWIDTH_64BITS`.
pub const RL_BUSWIDTH_64BITS: u32 = 0x08;

// C+ mode command register

/// `RL_CPLUSCMD_TXENB`: enable C+ transmit mode.
pub const RL_CPLUSCMD_TXENB: u32 = 0x0001;
/// `RL_CPLUSCMD_RXENB`: enable C+ receive mode.
pub const RL_CPLUSCMD_RXENB: u32 = 0x0002;
/// `RL_CPLUSCMD_PCI_MRW`: enable PCI multi-read/write.
pub const RL_CPLUSCMD_PCI_MRW: u32 = 0x0008;
/// `RL_CPLUSCMD_PCI_DAC`: PCI dual-address cycle only.
pub const RL_CPLUSCMD_PCI_DAC: u32 = 0x0010;
/// `RL_CPLUSCMD_RXCSUM_ENB`: enable RX checksum offload.
pub const RL_CPLUSCMD_RXCSUM_ENB: u32 = 0x0020;
/// `RL_CPLUSCMD_VLANSTRIP`: enable VLAN tag stripping.
pub const RL_CPLUSCMD_VLANSTRIP: u32 = 0x0040;
/// `RL_CPLUSCMD_MACSTAT_DIS`: 8168B/C/CP.
pub const RL_CPLUSCMD_MACSTAT_DIS: u32 = 0x0080;
/// `RL_CPLUSCMD_ASF`: 8168C/CP.
pub const RL_CPLUSCMD_ASF: u32 = 0x0100;
/// `RL_CPLUSCMD_DBG_SEL`: 8168C/CP.
pub const RL_CPLUSCMD_DBG_SEL: u32 = 0x0200;
/// `RL_CPLUSCMD_FORCE_TXFC`: 8168C/CP.
pub const RL_CPLUSCMD_FORCE_TXFC: u32 = 0x0400;
/// `RL_CPLUSCMD_FORCE_RXFC`: 8168C/CP.
pub const RL_CPLUSCMD_FORCE_RXFC: u32 = 0x0800;
/// `RL_CPLUSCMD_FORCE_HDPX`: 8168C/CP.
pub const RL_CPLUSCMD_FORCE_HDPX: u32 = 0x1000;
/// `RL_CPLUSCMD_NORMAL_MODE`: 8168C/CP.
pub const RL_CPLUSCMD_NORMAL_MODE: u32 = 0x2000;
/// `RL_CPLUSCMD_DBG_ENB`: 8168C/CP.
pub const RL_CPLUSCMD_DBG_ENB: u32 = 0x4000;
/// `RL_CPLUSCMD_BIST_ENB`: 8168C/CP.
pub const RL_CPLUSCMD_BIST_ENB: u32 = 0x8000;

// C+ early transmit threshold

/// `RL_EARLYTXTHRESH_CNT`: byte count times 8.
pub const RL_EARLYTXTHRESH_CNT: u32 = 0x003F;

/// `RL_PHYAR_PHYDATA`.
pub const RL_PHYAR_PHYDATA: u32 = 0x0000FFFF;
/// `RL_PHYAR_PHYREG`.
pub const RL_PHYAR_PHYREG: u32 = 0x001F0000;
/// `RL_PHYAR_BUSY`.
pub const RL_PHYAR_BUSY: u32 = 0x80000000;

/// `RL_GMEDIASTAT_FDX`: full duplex.
pub const RL_GMEDIASTAT_FDX: u32 = 0x01;
/// `RL_GMEDIASTAT_LINK`: link up.
pub const RL_GMEDIASTAT_LINK: u32 = 0x02;
/// `RL_GMEDIASTAT_10MBPS`: 10mps link.
pub const RL_GMEDIASTAT_10MBPS: u32 = 0x04;
/// `RL_GMEDIASTAT_100MBPS`: 100mbps link.
pub const RL_GMEDIASTAT_100MBPS: u32 = 0x08;
/// `RL_GMEDIASTAT_1000MBPS`: gigE link.
pub const RL_GMEDIASTAT_1000MBPS: u32 = 0x10;
/// `RL_GMEDIASTAT_RXFLOW`: RX flow control on.
pub const RL_GMEDIASTAT_RXFLOW: u32 = 0x20;
/// `RL_GMEDIASTAT_TXFLOW`: TX flow control on.
pub const RL_GMEDIASTAT_TXFLOW: u32 = 0x40;
/// `RL_GMEDIASTAT_TBI`: TBI enabled.
pub const RL_GMEDIASTAT_TBI: u32 = 0x80;

/// `RL_RX_BUF_SZ`.
pub const RL_RX_BUF_SZ: u32 = RL_RXBUF_64;
/// `RL_RXBUFLEN`.
pub const RL_RXBUFLEN: u32 = 1 << ((RL_RX_BUF_SZ >> 11) + 13);
/// `RL_TX_LIST_CNT`.
pub const RL_TX_LIST_CNT: u32 = 4;
/// `RL_MIN_FRAMELEN`.
pub const RL_MIN_FRAMELEN: u32 = 60;
/// `RL_TX_THRESH_INIT`.
pub const RL_TX_THRESH_INIT: u32 = 96;
/// `RL_RX_FIFOTHRESH`.
pub const RL_RX_FIFOTHRESH: u32 = RL_RXFIFO_NOTHRESH;
/// `RL_RX_MAXDMA`.
pub const RL_RX_MAXDMA: u32 = RL_RXDMA_UNLIMITED;
/// `RL_TX_MAXDMA`.
pub const RL_TX_MAXDMA: u32 = RL_TXDMA_2048BYTES;

/// `RL_RXCFG_CONFIG`.
pub const RL_RXCFG_CONFIG: u32 = RL_RX_FIFOTHRESH | RL_RX_MAXDMA | RL_RX_BUF_SZ;
/// `RL_TXCFG_CONFIG`.
pub const RL_TXCFG_CONFIG: u32 = RL_TXCFG_IFG | RL_TX_MAXDMA;

/// `RL_IM_MAGIC`.
pub const RL_IM_MAGIC: u32 = 0x5050;

/// `RL_TDESC_CMD_FRAGLEN`.
pub const RL_TDESC_CMD_FRAGLEN: u32 = 0x0000FFFF;
/// `RL_TDESC_CMD_TCPCSUM`: TCP checksum enable.
pub const RL_TDESC_CMD_TCPCSUM: u32 = 0x00010000;
/// `RL_TDESC_CMD_UDPCSUM`: UDP checksum enable.
pub const RL_TDESC_CMD_UDPCSUM: u32 = 0x00020000;
/// `RL_TDESC_CMD_IPCSUM`: IP header checksum enable.
pub const RL_TDESC_CMD_IPCSUM: u32 = 0x00040000;
/// `RL_TDESC_CMD_MSSVAL`: Large send MSS value.
pub const RL_TDESC_CMD_MSSVAL: u32 = 0x07FF0000;
/// `RL_TDESC_CMD_LGSEND`: TCP large send enb.
pub const RL_TDESC_CMD_LGSEND: u32 = 0x08000000;
/// `RL_TDESC_CMD_EOF`: end of frame marker.
pub const RL_TDESC_CMD_EOF: u32 = 0x10000000;
/// `RL_TDESC_CMD_SOF`: start of frame marker.
pub const RL_TDESC_CMD_SOF: u32 = 0x20000000;
/// `RL_TDESC_CMD_EOR`: end of ring marker.
pub const RL_TDESC_CMD_EOR: u32 = 0x40000000;
/// `RL_TDESC_CMD_OWN`: chip owns descriptor.
pub const RL_TDESC_CMD_OWN: u32 = 0x80000000;

/// `RL_TDESC_VLANCTL_TAG`: Insert VLAN tag.
pub const RL_TDESC_VLANCTL_TAG: u32 = 0x00020000;
/// `RL_TDESC_VLANCTL_DATA`: TAG data.
pub const RL_TDESC_VLANCTL_DATA: u32 = 0x0000FFFF;
// RTL8168C/RTL8168CP/RTL8111C/RTL8111CP
/// `RL_TDESC_CMD_IPCSUMV2`.
pub const RL_TDESC_CMD_IPCSUMV2: u32 = 0x20000000;
/// `RL_TDESC_CMD_TCPCSUMV2`.
pub const RL_TDESC_CMD_TCPCSUMV2: u32 = 0x40000000;
/// `RL_TDESC_CMD_UDPCSUMV2`.
pub const RL_TDESC_CMD_UDPCSUMV2: u32 = 0x80000000;

/// `RL_TDESC_STAT_COLCNT`: collision count.
pub const RL_TDESC_STAT_COLCNT: u32 = 0x000F0000;
/// `RL_TDESC_STAT_EXCESSCOL`: excessive collisions.
pub const RL_TDESC_STAT_EXCESSCOL: u32 = 0x00100000;
/// `RL_TDESC_STAT_LINKFAIL`: link failure.
pub const RL_TDESC_STAT_LINKFAIL: u32 = 0x00200000;
/// `RL_TDESC_STAT_OWINCOL`: out-of-window collision.
pub const RL_TDESC_STAT_OWINCOL: u32 = 0x00400000;
/// `RL_TDESC_STAT_TXERRSUM`: transmit error summary.
pub const RL_TDESC_STAT_TXERRSUM: u32 = 0x00800000;
/// `RL_TDESC_STAT_UNDERRUN`: TX underrun occurred.
pub const RL_TDESC_STAT_UNDERRUN: u32 = 0x02000000;
/// `RL_TDESC_STAT_OWN`.
pub const RL_TDESC_STAT_OWN: u32 = 0x80000000;

/// `RL_RDESC_CMD_EOR`.
pub const RL_RDESC_CMD_EOR: u32 = 0x40000000;
/// `RL_RDESC_CMD_OWN`.
pub const RL_RDESC_CMD_OWN: u32 = 0x80000000;
/// `RL_RDESC_CMD_BUFLEN`.
pub const RL_RDESC_CMD_BUFLEN: u32 = 0x00001FFF;

/// `RL_RDESC_STAT_OWN`.
pub const RL_RDESC_STAT_OWN: u32 = 0x80000000;
/// `RL_RDESC_STAT_EOR`.
pub const RL_RDESC_STAT_EOR: u32 = 0x40000000;
/// `RL_RDESC_STAT_SOF`.
pub const RL_RDESC_STAT_SOF: u32 = 0x20000000;
/// `RL_RDESC_STAT_EOF`.
pub const RL_RDESC_STAT_EOF: u32 = 0x10000000;
/// `RL_RDESC_STAT_FRALIGN`: frame alignment error.
pub const RL_RDESC_STAT_FRALIGN: u32 = 0x08000000;
/// `RL_RDESC_STAT_MCAST`: multicast pkt received.
pub const RL_RDESC_STAT_MCAST: u32 = 0x04000000;
/// `RL_RDESC_STAT_UCAST`: unicast pkt received.
pub const RL_RDESC_STAT_UCAST: u32 = 0x02000000;
/// `RL_RDESC_STAT_BCAST`: broadcast pkt received.
pub const RL_RDESC_STAT_BCAST: u32 = 0x01000000;
/// `RL_RDESC_STAT_BUFOFLOW`: out of buffer space.
pub const RL_RDESC_STAT_BUFOFLOW: u32 = 0x00800000;
/// `RL_RDESC_STAT_FIFOOFLOW`: FIFO overrun.
pub const RL_RDESC_STAT_FIFOOFLOW: u32 = 0x00400000;
/// `RL_RDESC_STAT_GIANT`: pkt > 4096 bytes.
pub const RL_RDESC_STAT_GIANT: u32 = 0x00200000;
/// `RL_RDESC_STAT_RXERRSUM`: RX error summary.
pub const RL_RDESC_STAT_RXERRSUM: u32 = 0x00100000;
/// `RL_RDESC_STAT_RUNT`: runt packet received.
pub const RL_RDESC_STAT_RUNT: u32 = 0x00080000;
/// `RL_RDESC_STAT_CRCERR`: CRC error.
pub const RL_RDESC_STAT_CRCERR: u32 = 0x00040000;
/// `RL_RDESC_STAT_PROTOID`: Protocol type.
pub const RL_RDESC_STAT_PROTOID: u32 = 0x00030000;
/// `RL_RDESC_STAT_UDP`: UDP, 8168C/CP, 8111C/CP.
pub const RL_RDESC_STAT_UDP: u32 = 0x00020000;
/// `RL_RDESC_STAT_TCP`: TCP, 8168C/CP, 8111C/CP.
pub const RL_RDESC_STAT_TCP: u32 = 0x00010000;
/// `RL_RDESC_STAT_IPSUMBAD`: IP header checksum bad.
pub const RL_RDESC_STAT_IPSUMBAD: u32 = 0x00008000;
/// `RL_RDESC_STAT_UDPSUMBAD`: UDP checksum bad.
pub const RL_RDESC_STAT_UDPSUMBAD: u32 = 0x00004000;
/// `RL_RDESC_STAT_TCPSUMBAD`: TCP checksum bad.
pub const RL_RDESC_STAT_TCPSUMBAD: u32 = 0x00002000;
/// `RL_RDESC_STAT_FRAGLEN`: RX'ed frame/frag len.
pub const RL_RDESC_STAT_FRAGLEN: u32 = 0x00001FFF;
/// `RL_RDESC_STAT_GFRAGLEN`: RX'ed frame/frag len.
pub const RL_RDESC_STAT_GFRAGLEN: u32 = 0x00003FFF;
/// `RL_RDESC_STAT_ERRS`.
pub const RL_RDESC_STAT_ERRS: u32 = RL_RDESC_STAT_GIANT | RL_RDESC_STAT_RUNT | RL_RDESC_STAT_CRCERR;

/// `RL_RDESC_VLANCTL_TAG`: VLAN tag available (rl_vlandata valid).
pub const RL_RDESC_VLANCTL_TAG: u32 = 0x00010000;
/// `RL_RDESC_VLANCTL_DATA`: TAG data.
pub const RL_RDESC_VLANCTL_DATA: u32 = 0x0000FFFF;
// RTL8168C/RTL8168CP/RTL8111C/RTL8111CP
/// `RL_RDESC_IPV6`.
pub const RL_RDESC_IPV6: u32 = 0x80000000;
/// `RL_RDESC_IPV4`.
pub const RL_RDESC_IPV4: u32 = 0x40000000;

/// `RL_PROTOID_NONIP`.
pub const RL_PROTOID_NONIP: u32 = 0x00000000;
/// `RL_PROTOID_TCPIP`.
pub const RL_PROTOID_TCPIP: u32 = 0x00010000;
/// `RL_PROTOID_UDPIP`.
pub const RL_PROTOID_UDPIP: u32 = 0x00020000;
/// `RL_PROTOID_IP`.
pub const RL_PROTOID_IP: u32 = 0x00030000;

/// `RL_8169_TX_DESC_CNT`.
pub const RL_8169_TX_DESC_CNT: u32 = 1024;
/// `RL_8169_RX_DESC_CNT`.
pub const RL_8169_RX_DESC_CNT: u32 = 1024;
/// `RL_8139_TX_DESC_CNT`.
pub const RL_8139_TX_DESC_CNT: u32 = 64;
/// `RL_8139_RX_DESC_CNT`.
pub const RL_8139_RX_DESC_CNT: u32 = 64;
/// `RL_TX_DESC_CNT`.
pub const RL_TX_DESC_CNT: u32 = RL_8169_TX_DESC_CNT;
/// `RL_RX_DESC_CNT`.
pub const RL_RX_DESC_CNT: u32 = RL_8169_RX_DESC_CNT;
/// `RL_8169_NTXSEGS`.
pub const RL_8169_NTXSEGS: u32 = 32;
/// `RL_8139_NTXSEGS`.
pub const RL_8139_NTXSEGS: u32 = 8;

/// `RL_TX_QLEN`.
pub const RL_TX_QLEN: u32 = 64;

/// `RL_RING_ALIGN`.
pub const RL_RING_ALIGN: u32 = 256;
/// `RE_ETHER_ALIGN`: 2 with `__STRICT_ALIGNMENT`, so the IP header of a received frame is
/// aligned; 0 otherwise.
pub const RE_ETHER_ALIGN: u32 = if STRICT_ALIGNMENT { 2 } else { 0 };
/// `RE_RX_DESC_BUFLEN`.
pub const RE_RX_DESC_BUFLEN: u32 = MCLBYTES as u32 - RE_ETHER_ALIGN;

/// `RL_JUMBO_FRAMELEN`.
pub const RL_JUMBO_FRAMELEN: u32 = 9 * 1024;
/// `RL_JUMBO_MTU_4K`.
pub const RL_JUMBO_MTU_4K: u32 =
    (4 * 1024) - (ETHER_HDR_LEN as u32) - (ETHER_CRC_LEN as u32) - (ETHER_VLAN_ENCAP_LEN as u32);
/// `RL_JUMBO_MTU_6K`.
pub const RL_JUMBO_MTU_6K: u32 =
    (6 * 1024) - (ETHER_HDR_LEN as u32) - (ETHER_CRC_LEN as u32) - (ETHER_VLAN_ENCAP_LEN as u32);
/// `RL_JUMBO_MTU_7K`.
pub const RL_JUMBO_MTU_7K: u32 =
    (7 * 1024) - (ETHER_HDR_LEN as u32) - (ETHER_CRC_LEN as u32) - (ETHER_VLAN_ENCAP_LEN as u32);
/// `RL_JUMBO_MTU_9K`.
pub const RL_JUMBO_MTU_9K: u32 =
    (9 * 1024) - (ETHER_HDR_LEN as u32) - (ETHER_CRC_LEN as u32) - (ETHER_VLAN_ENCAP_LEN as u32);
/// `RL_MTU`.
pub const RL_MTU: u32 = ETHERMTU as u32;

/// `MAX_NUM_MULTICAST_ADDRESSES`.
pub const MAX_NUM_MULTICAST_ADDRESSES: u32 = 128;

/// `RL_MII_STARTDELIM`.
pub const RL_MII_STARTDELIM: u32 = 0x01;
/// `RL_MII_READOP`.
pub const RL_MII_READOP: u32 = 0x02;
/// `RL_MII_WRITEOP`.
pub const RL_MII_WRITEOP: u32 = 0x01;
/// `RL_MII_TURNAROUND`.
pub const RL_MII_TURNAROUND: u32 = 0x02;

/// `RL_UNKNOWN`.
pub const RL_UNKNOWN: u32 = 0;
/// `RL_8129`.
pub const RL_8129: u32 = 1;
/// `RL_8139`.
pub const RL_8139: u32 = 2;

/// `RL_FLAG_MSI`.
pub const RL_FLAG_MSI: u32 = 0x00000001;
/// `RL_FLAG_PCI64`.
pub const RL_FLAG_PCI64: u32 = 0x00000002;
/// `RL_FLAG_PCIE`.
pub const RL_FLAG_PCIE: u32 = 0x00000004;
/// `RL_FLAG_PHYWAKE`.
pub const RL_FLAG_PHYWAKE: u32 = 0x00000008;
/// `RL_FLAG_PAR`.
pub const RL_FLAG_PAR: u32 = 0x00000010;
/// `RL_FLAG_DESCV2`.
pub const RL_FLAG_DESCV2: u32 = 0x00000020;
/// `RL_FLAG_MACSTAT`.
pub const RL_FLAG_MACSTAT: u32 = 0x00000040;
/// `RL_FLAG_HWIM`.
pub const RL_FLAG_HWIM: u32 = 0x00000080;
/// `RL_FLAG_TIMERINTR`.
pub const RL_FLAG_TIMERINTR: u32 = 0x00000100;
/// `RL_FLAG_MACRESET`.
pub const RL_FLAG_MACRESET: u32 = 0x00000200;
/// `RL_FLAG_CMDSTOP`.
pub const RL_FLAG_CMDSTOP: u32 = 0x00000400;
/// `RL_FLAG_MACSLEEP`.
pub const RL_FLAG_MACSLEEP: u32 = 0x00000800;
/// `RL_FLAG_AUTOPAD`.
pub const RL_FLAG_AUTOPAD: u32 = 0x00001000;
/// `RL_FLAG_LINK`.
pub const RL_FLAG_LINK: u32 = 0x00002000;
/// `RL_FLAG_PHYWAKE_PM`.
pub const RL_FLAG_PHYWAKE_PM: u32 = 0x00004000;
/// `RL_FLAG_EARLYOFF`.
pub const RL_FLAG_EARLYOFF: u32 = 0x00008000;
/// `RL_FLAG_EARLYOFFV2`.
pub const RL_FLAG_EARLYOFFV2: u32 = 0x00010000;
/// `RL_FLAG_RXDV_GATED`.
pub const RL_FLAG_RXDV_GATED: u32 = 0x00020000;
/// `RL_FLAG_FASTETHER`.
pub const RL_FLAG_FASTETHER: u32 = 0x00040000;
/// `RL_FLAG_CMDSTOP_WAIT_TXQ`.
pub const RL_FLAG_CMDSTOP_WAIT_TXQ: u32 = 0x00080000;
/// `RL_FLAG_JUMBOV2`.
pub const RL_FLAG_JUMBOV2: u32 = 0x00100000;
/// `RL_FLAG_WOL_MANLINK`.
pub const RL_FLAG_WOL_MANLINK: u32 = 0x00200000;
/// `RL_FLAG_WAIT_TXPOLL`.
pub const RL_FLAG_WAIT_TXPOLL: u32 = 0x00400000;
/// `RL_FLAG_WOLRXENB`.
pub const RL_FLAG_WOLRXENB: u32 = 0x00800000;

/// `RL_IMTYPE_NONE`.
pub const RL_IMTYPE_NONE: u32 = 0;
/// `RL_IMTYPE_SIM`: simulated.
pub const RL_IMTYPE_SIM: u32 = 1;
/// `RL_IMTYPE_HW`: hardware based.
pub const RL_IMTYPE_HW: u32 = 2;

/// `RL_IP4CSUMTX_MINLEN`.
pub const RL_IP4CSUMTX_MINLEN: u32 = 28;
/// `RL_IP4CSUMTX_PADLEN`.
pub const RL_IP4CSUMTX_PADLEN: u32 = (ETHER_HDR_LEN as u32) + RL_IP4CSUMTX_MINLEN;

/// `RL_TIMEOUT`.
pub const RL_TIMEOUT: u32 = 1000;
/// `RL_PHY_TIMEOUT`.
pub const RL_PHY_TIMEOUT: u32 = 20;

/// `RT_VENDORID`.
pub const RT_VENDORID: u32 = 0x10EC;

/// `RT_DEVICEID_8129`.
pub const RT_DEVICEID_8129: u32 = 0x8129;
/// `RT_DEVICEID_8101E`.
pub const RT_DEVICEID_8101E: u32 = 0x8136;
/// `RT_DEVICEID_8138`.
pub const RT_DEVICEID_8138: u32 = 0x8138;
/// `RT_DEVICEID_8139`.
pub const RT_DEVICEID_8139: u32 = 0x8139;
/// `RT_DEVICEID_8169SC`.
pub const RT_DEVICEID_8169SC: u32 = 0x8167;
/// `RT_DEVICEID_8168`.
pub const RT_DEVICEID_8168: u32 = 0x8168;
/// `RT_DEVICEID_8169`.
pub const RT_DEVICEID_8169: u32 = 0x8169;
/// `RT_DEVICEID_8100`.
pub const RT_DEVICEID_8100: u32 = 0x8100;

/// `ACCTON_VENDORID`.
pub const ACCTON_VENDORID: u32 = 0x1113;

/// `ACCTON_DEVICEID_5030`.
pub const ACCTON_DEVICEID_5030: u32 = 0x1211;

/// `DELTA_VENDORID`.
pub const DELTA_VENDORID: u32 = 0x1500;

/// `DELTA_DEVICEID_8139`.
pub const DELTA_DEVICEID_8139: u32 = 0x1360;

/// `ADDTRON_VENDORID`.
pub const ADDTRON_VENDORID: u32 = 0x4033;

/// `ADDTRON_DEVICEID_8139`.
pub const ADDTRON_DEVICEID_8139: u32 = 0x1360;

// D-Link Vendor ID
/// `DLINK_VENDORID`.
pub const DLINK_VENDORID: u32 = 0x1186;

// D-Link device IDs
/// `DLINK_DEVICEID_8139`.
pub const DLINK_DEVICEID_8139: u32 = 0x1300;
/// `DLINK_DEVICEID_8139_2`.
pub const DLINK_DEVICEID_8139_2: u32 = 0x1340;

// Abocom device IDs
/// `ABOCOM_DEVICEID_8139`.
pub const ABOCOM_DEVICEID_8139: u32 = 0xab06;

/// `RL_PCI_VENDOR_ID`.
pub const RL_PCI_VENDOR_ID: u32 = 0x00;
/// `RL_PCI_DEVICE_ID`.
pub const RL_PCI_DEVICE_ID: u32 = 0x02;
/// `RL_PCI_COMMAND`.
pub const RL_PCI_COMMAND: u32 = 0x04;
/// `RL_PCI_STATUS`.
pub const RL_PCI_STATUS: u32 = 0x06;
/// `RL_PCI_CLASSCODE`.
pub const RL_PCI_CLASSCODE: u32 = 0x09;
/// `RL_PCI_LATENCY_TIMER`.
pub const RL_PCI_LATENCY_TIMER: u32 = 0x0D;
/// `RL_PCI_HEADER_TYPE`.
pub const RL_PCI_HEADER_TYPE: u32 = 0x0E;
/// `RL_PCI_LOIO`.
pub const RL_PCI_LOIO: u32 = 0x10;
/// `RL_PCI_LOMEM`.
pub const RL_PCI_LOMEM: u32 = 0x14;
/// `RL_PCI_LOMEM64`.
pub const RL_PCI_LOMEM64: u32 = 0x18;
/// `RL_PCI_BIOSROM`.
pub const RL_PCI_BIOSROM: u32 = 0x30;
/// `RL_PCI_INTLINE`.
pub const RL_PCI_INTLINE: u32 = 0x3C;
/// `RL_PCI_INTPIN`.
pub const RL_PCI_INTPIN: u32 = 0x3D;
/// `RL_PCI_MINGNT`.
pub const RL_PCI_MINGNT: u32 = 0x3E;
/// `RL_PCI_MINLAT`.
pub const RL_PCI_MINLAT: u32 = 0x0F;
/// `RL_PCI_PMCSR`.
pub const RL_PCI_PMCSR: u32 = 0x44;
/// `RL_PCI_RESETOPT`.
pub const RL_PCI_RESETOPT: u32 = 0x48;
/// `RL_PCI_EEPROM_DATA`.
pub const RL_PCI_EEPROM_DATA: u32 = 0x4C;

/// `RL_PCI_CAPID`: 8 bits.
pub const RL_PCI_CAPID: u32 = 0x50;
/// `RL_PCI_NEXTPTR`: 8 bits.
pub const RL_PCI_NEXTPTR: u32 = 0x51;
/// `RL_PCI_PWRMGMTCAP`: 16 bits.
pub const RL_PCI_PWRMGMTCAP: u32 = 0x52;
/// `RL_PCI_PWRMGMTCTRL`: 16 bits.
pub const RL_PCI_PWRMGMTCTRL: u32 = 0x54;

/// `RL_PSTATE_MASK`.
pub const RL_PSTATE_MASK: u32 = 0x0003;
/// `RL_PSTATE_D0`.
pub const RL_PSTATE_D0: u32 = 0x0000;
/// `RL_PSTATE_D1`.
pub const RL_PSTATE_D1: u32 = 0x0001;
/// `RL_PSTATE_D2`.
pub const RL_PSTATE_D2: u32 = 0x0002;
/// `RL_PSTATE_D3`.
pub const RL_PSTATE_D3: u32 = 0x0003;
/// `RL_PME_EN`.
pub const RL_PME_EN: u32 = 0x0100;
/// `RL_PME_STATUS`.
pub const RL_PME_STATUS: u32 = 0x8000;

/// `struct rl_chain_data`: rl(4)'s receive buffer and its four transmit slots (the 8129 and
/// 8139 without C+ mode).
pub struct RlChainData {
    /// `cur_rx`.
    pub cur_rx: Cell<u16>,
    /// `rl_rx_buf`.
    pub rl_rx_buf: Cell<*mut u8>,
    /// `rl_rx_buf_ptr`.
    pub rl_rx_buf_ptr: Cell<*mut u8>,
    /// `rl_rx_buf_pa`.
    pub rl_rx_buf_pa: Cell<BusAddr>,
    /// `rl_tx_chain`.
    pub rl_tx_chain: [Cell<Option<&'static Mbuf>>; RL_TX_LIST_CNT as usize],
    /// `rl_tx_dmamap`.
    pub rl_tx_dmamap: [Cell<Option<&'static BusDmamap>>; RL_TX_LIST_CNT as usize],
    /// `last_tx`.
    pub last_tx: Cell<u8>,
    /// `cur_tx`.
    pub cur_tx: Cell<u8>,
}

/// `struct rl_desc`: RX/TX descriptor definition. When large send mode is enabled, the lower
/// 11 bits of the TX rl_cmd word are used to hold the MSS, and the checksum offload bits are
/// disabled. The structure layout is the same for RX and TX descriptors. Little-endian in
/// memory.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RlDesc {
    /// `rl_cmdstat`.
    pub rl_cmdstat: u32,
    /// `rl_vlanctl`.
    pub rl_vlanctl: u32,
    /// `rl_bufaddr_lo`.
    pub rl_bufaddr_lo: u32,
    /// `rl_bufaddr_hi`.
    pub rl_bufaddr_hi: u32,
}

/// `struct re_stats`: statistics counter structure (8139C+ and 8169 only).
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct ReStats {
    /// `re_tx_ok`.
    pub re_tx_ok: u64,
    /// `re_rx_ok`.
    pub re_rx_ok: u64,
    /// `re_tx_er`.
    pub re_tx_er: u64,
    /// `re_rx_er`.
    pub re_rx_er: u32,
    /// `re_miss_pkt`.
    pub re_miss_pkt: u16,
    /// `re_fae`.
    pub re_fae: u16,
    /// `re_tx_1col`.
    pub re_tx_1col: u32,
    /// `re_tx_mcol`.
    pub re_tx_mcol: u32,
    /// `re_rx_ok_phy`.
    pub re_rx_ok_phy: u64,
    /// `re_rx_ok_brd`.
    pub re_rx_ok_brd: u64,
    /// `re_rx_ok_mul`.
    pub re_rx_ok_mul: u32,
    /// `re_tx_abt`.
    pub re_tx_abt: u16,
    /// `re_tx_undrn`.
    pub re_tx_undrn: u16,
}

/// `struct rl_type`.
#[derive(Clone, Copy, Debug)]
pub struct RlType {
    /// `rl_vid`.
    pub rl_vid: u16,
    /// `rl_did`.
    pub rl_did: u16,
}

/// `struct rl_mii_frame`: an MII frame clocked by hand (rl(4)'s 8129).
#[derive(Clone, Copy, Debug, Default)]
pub struct RlMiiFrame {
    /// `mii_stdelim`.
    pub mii_stdelim: u8,
    /// `mii_opcode`.
    pub mii_opcode: u8,
    /// `mii_phyaddr`.
    pub mii_phyaddr: u8,
    /// `mii_regaddr`.
    pub mii_regaddr: u8,
    /// `mii_turnaround`.
    pub mii_turnaround: u8,
    /// `mii_data`.
    pub mii_data: u16,
}

/// `struct rl_rxsoft`: a receive descriptor's mbuf and DMA map.
pub struct RlRxsoft {
    /// `rxs_mbuf`.
    pub rxs_mbuf: Cell<Option<&'static Mbuf>>,
    /// `rxs_dmamap`.
    pub rxs_dmamap: Cell<Option<&'static BusDmamap>>,
}

impl RlRxsoft {
    /// `rxs->rxs_dmamap`, which `re_attach` created.
    pub fn map(&self) -> &'static BusDmamap {
        match self.rxs_dmamap.get() {
            Some(m) => m,
            None => panic(format_args!("re: rx slot without a dma map")),
        }
    }
}

/// `struct rl_txq`: a transmit slot (indexed by its first descriptor).
pub struct RlTxq {
    /// `txq_mbuf`.
    pub txq_mbuf: Cell<Option<&'static Mbuf>>,
    /// `txq_dmamap`.
    pub txq_dmamap: Cell<Option<&'static BusDmamap>>,
    /// `txq_descidx`: the frame's last descriptor.
    pub txq_descidx: Cell<i32>,
    /// `txq_nsegs`.
    pub txq_nsegs: Cell<i32>,
}

impl RlTxq {
    /// `txq->txq_dmamap`, which `re_attach` created.
    pub fn map(&self) -> &'static BusDmamap {
        match self.txq_dmamap.get() {
            Some(m) => m,
            None => panic(format_args!("re: tx slot without a dma map")),
        }
    }
}

/// `struct rl_list_data`: the C+ descriptor rings and their slots.
///
/// The transmit slots and descriptors are filled by `re_start` (serialised by the send
/// queue) from `rl_txq_prodidx` and reclaimed by `re_txeof` (the interrupt) from
/// `rl_txq_considx`: each slot belongs to one side at a time, as in C. The receive side is
/// the interrupt's, and `re_init`/`re_stop`'s under the kernel lock with the interrupt
/// barred.
pub struct RlListData {
    /// `rl_txq`.
    pub rl_txq: [RlTxq; RL_TX_DESC_CNT as usize],
    /// `rl_txq_considx`.
    pub rl_txq_considx: AtomicU32,
    /// `rl_txq_prodidx`.
    pub rl_txq_prodidx: AtomicU32,

    /// `rl_tx_list_map`.
    pub rl_tx_list_map: Cell<Option<&'static BusDmamap>>,
    /// `rl_tx_list`.
    pub rl_tx_list: Cell<*mut RlDesc>,
    /// `rl_tx_free`: # of free descriptors.
    pub rl_tx_free: Cell<i32>,
    /// `rl_tx_nextfree`: next descriptor to use.
    pub rl_tx_nextfree: Cell<i32>,
    /// `rl_tx_desc_cnt`: # of descriptors.
    pub rl_tx_desc_cnt: Cell<i32>,
    /// `rl_tx_ndescs`: descs per tx packet.
    pub rl_tx_ndescs: Cell<i32>,
    /// `rl_tx_listseg`.
    pub rl_tx_listseg: Cell<BusDmaSegment>,
    /// `rl_tx_listnseg`.
    pub rl_tx_listnseg: Cell<i32>,

    /// `rl_rxsoft`.
    pub rl_rxsoft: [RlRxsoft; RL_RX_DESC_CNT as usize],
    /// `rl_rx_list_map`.
    pub rl_rx_list_map: Cell<Option<&'static BusDmamap>>,
    /// `rl_rx_list`.
    pub rl_rx_list: Cell<*mut RlDesc>,
    /// `rl_rx_considx`.
    pub rl_rx_considx: Cell<i32>,
    /// `rl_rx_prodidx`.
    pub rl_rx_prodidx: Cell<i32>,
    /// `rl_rx_desc_cnt`: # of descriptors.
    pub rl_rx_desc_cnt: Cell<i32>,
    /// `rl_rx_ring`.
    pub rl_rx_ring: Cell<IfRxring>,
    /// `rl_rx_listseg`.
    pub rl_rx_listseg: Cell<BusDmaSegment>,
    /// `rl_rx_listnseg`.
    pub rl_rx_listnseg: Cell<i32>,
}

impl RlListData {
    /// `ldata->rl_tx_list_map`, which `re_attach` created.
    pub fn tx_list_map(&self) -> &'static BusDmamap {
        match self.rl_tx_list_map.get() {
            Some(m) => m,
            None => panic(format_args!("re: no tx list map")),
        }
    }

    /// `ldata->rl_rx_list_map`, which `re_attach` created.
    pub fn rx_list_map(&self) -> &'static BusDmamap {
        match self.rl_rx_list_map.get() {
            Some(m) => m,
            None => panic(format_args!("re: no rx list map")),
        }
    }

    /// `&ldata->rl_txq[i]`.
    pub fn txq(&self, i: u32) -> &RlTxq {
        match self.rl_txq.get(i as usize) {
            Some(q) => q,
            None => panic(format_args!("re: bad tx slot {i}")),
        }
    }

    /// `&ldata->rl_rxsoft[i]`.
    pub fn rxsoft(&self, i: u32) -> &RlRxsoft {
        match self.rl_rxsoft.get(i as usize) {
            Some(r) => r,
            None => panic(format_args!("re: bad rx slot {i}")),
        }
    }

    /// `ldata->rl_rx_ring`, changed by `f`.
    pub fn with_rx_ring<R>(&self, f: impl FnOnce(&mut IfRxring) -> R) -> R {
        let mut ring = self.rl_rx_ring.get();
        let r = f(&mut ring);
        self.rl_rx_ring.set(ring);
        r
    }

    /// `&ldata->rl_tx_list[i]`, a descriptor of the mapped transmit ring.
    fn txd(&self, i: u32) -> *mut RlDesc {
        let base = self.rl_tx_list.get();
        if base.is_null() || i >= self.rl_tx_desc_cnt.get() as u32 {
            panic(format_args!("re: bad tx descriptor {i}"));
        }
        // SAFETY: re_attach mapped `rl_tx_desc_cnt` descriptors at `base`.
        unsafe { base.add(i as usize) }
    }

    /// `ldata->rl_tx_list[i]`, read whole as the controller left it.
    pub fn txd_get(&self, i: u32) -> RlDesc {
        // SAFETY: a descriptor of the mapped ring (`txd`); plain data.
        unsafe { ptr::read_volatile(self.txd(i)) }
    }

    /// `ldata->rl_tx_list[i] = d`.
    pub fn txd_set(&self, i: u32, d: RlDesc) {
        // SAFETY: a descriptor of the mapped ring (`txd`).
        unsafe { ptr::write_volatile(self.txd(i), d) }
    }

    /// `&ldata->rl_rx_list[i]`, a descriptor of the mapped receive ring.
    fn rxd(&self, i: u32) -> *mut RlDesc {
        let base = self.rl_rx_list.get();
        if base.is_null() || i >= self.rl_rx_desc_cnt.get() as u32 {
            panic(format_args!("re: bad rx descriptor {i}"));
        }
        // SAFETY: re_attach mapped `rl_rx_desc_cnt` descriptors at `base`.
        unsafe { base.add(i as usize) }
    }

    /// `ldata->rl_rx_list[i]`, read whole as the controller wrote it back.
    pub fn rxd_get(&self, i: u32) -> RlDesc {
        // SAFETY: a descriptor of the mapped ring (`rxd`); plain data.
        unsafe { ptr::read_volatile(self.rxd(i)) }
    }

    /// `ldata->rl_rx_list[i] = d`.
    pub fn rxd_set(&self, i: u32, d: RlDesc) {
        // SAFETY: a descriptor of the mapped ring (`rxd`).
        unsafe { ptr::write_volatile(self.rxd(i), d) }
    }
}

/// `struct rl_softc`: the softc of rl(4) and re(4).
#[repr(C)]
pub struct RlSoftc {
    /// `sc_dev`: us, as a device.
    pub sc_dev: Device,
    /// `sc_ih`: interrupt vectoring.
    pub sc_ih: Cell<*mut c_void>,
    /// `rl_bhandle`: bus space handle.
    pub rl_bhandle: Cell<Option<BusSpaceHandle>>,
    /// `rl_btag`: bus space tag.
    pub rl_btag: Cell<Option<BusSpaceTag>>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_rx_seg`.
    pub sc_rx_seg: Cell<BusDmaSegment>,
    /// `sc_rx_dmamap`.
    pub sc_rx_dmamap: Cell<Option<&'static BusDmamap>>,
    /// `sc_arpcom`: interface info.
    pub sc_arpcom: Arpcom,
    /// `sc_mii`: MII information.
    pub sc_mii: MiiData,
    /// `rl_type`.
    pub rl_type: Cell<u8>,
    /// `sc_hwrev`.
    pub sc_hwrev: Cell<u32>,
    /// `sc_product`.
    pub sc_product: Cell<u16>,
    /// `rl_max_mtu`.
    pub rl_max_mtu: Cell<i32>,
    /// `rl_eecmd_read`.
    pub rl_eecmd_read: Cell<i32>,
    /// `rl_eewidth`.
    pub rl_eewidth: Cell<i32>,
    /// `rl_bus_speed`.
    pub rl_bus_speed: Cell<i32>,
    /// `rl_txthresh`.
    pub rl_txthresh: Cell<i32>,
    /// `rl_cfg0`.
    pub rl_cfg0: Cell<u32>,
    /// `rl_cfg1`.
    pub rl_cfg1: Cell<u32>,
    /// `rl_cfg2`.
    pub rl_cfg2: Cell<u32>,
    /// `rl_cfg3`.
    pub rl_cfg3: Cell<u32>,
    /// `rl_cfg4`.
    pub rl_cfg4: Cell<u32>,
    /// `rl_cfg5`.
    pub rl_cfg5: Cell<u32>,
    /// `rl_cdata`.
    pub rl_cdata: RlChainData,
    /// `sc_tick_tmo`.
    pub sc_tick_tmo: Timeout,

    /// `rl_ldata`.
    pub rl_ldata: RlListData,
    /// `rl_head`.
    pub rl_head: Cell<Option<&'static Mbuf>>,
    /// `rl_tail`.
    pub rl_tail: Cell<Option<&'static Mbuf>>,
    /// `rl_rxlenmask`.
    pub rl_rxlenmask: Cell<u32>,
    /// `timer_handle`.
    pub timer_handle: Timeout,
    /// `rl_start`.
    pub rl_start: Task,

    /// `rl_txstart`: the transmit poll register (`RL_TXSTART` or `RL_GTXSTART`).
    pub rl_txstart: Cell<u32>,
    /// `rl_flags` (`RL_FLAG_*`).
    pub rl_flags: AtomicU32,

    /// `rl_intrs`.
    pub rl_intrs: Cell<u16>,
    /// `rl_tx_ack`.
    pub rl_tx_ack: Cell<u16>,
    /// `rl_rx_ack`.
    pub rl_rx_ack: Cell<u16>,
    /// `rl_tx_time`.
    pub rl_tx_time: Cell<i32>,
    /// `rl_rx_time`.
    pub rl_rx_time: Cell<i32>,
    /// `rl_sim_time`.
    pub rl_sim_time: Cell<i32>,
    /// `rl_imtype` (`RL_IMTYPE_*`).
    pub rl_imtype: Cell<i32>,
    /// `rl_timerintr`.
    pub rl_timerintr: Cell<i32>,
    // rl_kstat: kstat(4) is not configured (NKSTAT 0).
}

impl RlSoftc {
    /// `sc->sc_dev.dv_xname`.
    pub fn devname(&self) -> &str {
        self.sc_dev.xname()
    }

    /// `sc->sc_dmat`, which the bus front-end sets before the attach.
    pub fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => panic(format_args!("{}: no DMA tag", self.devname())),
        }
    }

    /// `sc->rl_btag` and `sc->rl_bhandle`, which the bus front-end maps first.
    pub fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.rl_btag.get(), self.rl_bhandle.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("{}: registers not mapped", self.devname())),
        }
    }

    /// `sc->rl_flags`.
    pub fn flags(&self) -> u32 {
        self.rl_flags.load(Ordering::Relaxed)
    }

    /// `sc->rl_flags |= f`.
    pub fn set_flags(&self, f: u32) {
        self.rl_flags.fetch_or(f, Ordering::Relaxed);
    }

    /// `sc->rl_flags &= ~f`.
    pub fn clr_flags(&self, f: u32) {
        self.rl_flags.fetch_and(!f, Ordering::Relaxed);
    }

    /// `CSR_WRITE_RAW_4(sc, csr, val)`.
    pub fn csr_write_raw_4(&self, csr: u32, val: &[u8; 4]) {
        let (t, h) = self.regs();
        bus_space_write_4(t, h, csr as BusSize, u32::from_ne_bytes(*val));
    }

    /// `CSR_WRITE_4(sc, csr, val)`.
    pub fn csr_write_4(&self, csr: u32, val: u32) {
        let (t, h) = self.regs();
        bus_space_write_4(t, h, csr as BusSize, val);
    }

    /// `CSR_WRITE_2(sc, csr, val)`.
    pub fn csr_write_2(&self, csr: u32, val: u32) {
        let (t, h) = self.regs();
        bus_space_write_2(t, h, csr as BusSize, val as u16);
    }

    /// `CSR_WRITE_1(sc, csr, val)`.
    pub fn csr_write_1(&self, csr: u32, val: u32) {
        let (t, h) = self.regs();
        bus_space_write_1(t, h, csr as BusSize, val as u8);
    }

    /// `CSR_READ_4(sc, csr)`.
    pub fn csr_read_4(&self, csr: u32) -> u32 {
        let (t, h) = self.regs();
        bus_space_read_4(t, h, csr as BusSize)
    }

    /// `CSR_READ_2(sc, csr)`.
    pub fn csr_read_2(&self, csr: u32) -> u32 {
        let (t, h) = self.regs();
        u32::from(bus_space_read_2(t, h, csr as BusSize))
    }

    /// `CSR_READ_1(sc, csr)`.
    pub fn csr_read_1(&self, csr: u32) -> u32 {
        let (t, h) = self.regs();
        u32::from(bus_space_read_1(t, h, csr as BusSize))
    }

    /// `CSR_SETBIT_1(sc, offset, val)`.
    pub fn csr_setbit_1(&self, offset: u32, val: u32) {
        self.csr_write_1(offset, self.csr_read_1(offset) | val);
    }

    /// `CSR_CLRBIT_1(sc, offset, val)`.
    pub fn csr_clrbit_1(&self, offset: u32, val: u32) {
        self.csr_write_1(offset, self.csr_read_1(offset) & !val);
    }

    /// `CSR_SETBIT_2(sc, offset, val)`.
    pub fn csr_setbit_2(&self, offset: u32, val: u32) {
        self.csr_write_2(offset, self.csr_read_2(offset) | val);
    }

    /// `CSR_CLRBIT_2(sc, offset, val)`.
    pub fn csr_clrbit_2(&self, offset: u32, val: u32) {
        self.csr_write_2(offset, self.csr_read_2(offset) & !val);
    }

    /// `CSR_SETBIT_4(sc, offset, val)`.
    pub fn csr_setbit_4(&self, offset: u32, val: u32) {
        self.csr_write_4(offset, self.csr_read_4(offset) | val);
    }

    /// `CSR_CLRBIT_4(sc, offset, val)`.
    pub fn csr_clrbit_4(&self, offset: u32, val: u32) {
        self.csr_write_4(offset, self.csr_read_4(offset) & !val);
    }

    /// `RL_TX_LIST_SZ(sc)`.
    pub fn rl_tx_list_sz(&self) -> BusSize {
        self.rl_ldata.rl_tx_desc_cnt.get() as BusSize * size_of::<RlDesc>()
    }

    /// `RL_RX_LIST_SZ(sc)`.
    pub fn rl_rx_list_sz(&self) -> BusSize {
        self.rl_ldata.rl_rx_desc_cnt.get() as BusSize * size_of::<RlDesc>()
    }

    /// `RL_NEXT_TX_DESC(sc, x)`.
    pub fn rl_next_tx_desc(&self, x: u32) -> u32 {
        (x + 1) % self.rl_ldata.rl_tx_desc_cnt.get() as u32
    }

    /// `RL_NEXT_RX_DESC(sc, x)`.
    pub fn rl_next_rx_desc(&self, x: u32) -> u32 {
        (x + 1) % self.rl_ldata.rl_rx_desc_cnt.get() as u32
    }

    /// `RL_TXDESCSYNC(sc, idx, ops)`.
    pub fn rl_txdescsync(&self, idx: u32, ops: i32) {
        bus_dmamap_sync(
            self.dmat(),
            self.rl_ldata.tx_list_map(),
            size_of::<RlDesc>() * idx as usize,
            size_of::<RlDesc>(),
            ops,
        );
    }

    /// `RL_RXDESCSYNC(sc, idx, ops)`.
    pub fn rl_rxdescsync(&self, idx: u32, ops: i32) {
        bus_dmamap_sync(
            self.dmat(),
            self.rl_ldata.rx_list_map(),
            size_of::<RlDesc>() * idx as usize,
            size_of::<RlDesc>(),
            ops,
        );
    }

    /// `RL_RX_DMAMEM_SZ(sc)`: we are allocating pad DMA buffer after RX DMA descs for now
    /// because RL_TX_LIST_SZ(sc) always occupies whole page but RL_RX_LIST_SZ is less than
    /// PAGE_SIZE so there is some unused region.
    pub fn rl_rx_dmamem_sz(&self) -> BusSize {
        self.rl_rx_list_sz() + RL_IP4CSUMTX_PADLEN as BusSize
    }

    /// `RL_TXPADOFF(sc)`.
    pub fn rl_txpadoff(&self) -> BusSize {
        self.rl_rx_list_sz()
    }

    /// `RL_TXPADDADDR(sc)`.
    pub fn rl_txpaddaddr(&self) -> BusAddr {
        self.rl_ldata.rx_list_map().dm_segs()[0].get().ds_addr + self.rl_txpadoff()
    }

    /// `RL_CUR_TXADDR(x)`.
    pub fn rl_cur_txaddr(&self) -> u32 {
        u32::from(self.rl_cdata.cur_tx.get()) * 4 + RL_TXADDR0
    }

    /// `RL_CUR_TXSTAT(x)`.
    pub fn rl_cur_txstat(&self) -> u32 {
        u32::from(self.rl_cdata.cur_tx.get()) * 4 + RL_TXSTAT0
    }

    /// `RL_CUR_TXMBUF(x)`.
    pub fn rl_cur_txmbuf(&self) -> &Cell<Option<&'static Mbuf>> {
        &self.rl_cdata.rl_tx_chain[usize::from(self.rl_cdata.cur_tx.get()) % 4]
    }

    /// `RL_CUR_TXMAP(x)`.
    pub fn rl_cur_txmap(&self) -> &Cell<Option<&'static BusDmamap>> {
        &self.rl_cdata.rl_tx_dmamap[usize::from(self.rl_cdata.cur_tx.get()) % 4]
    }

    /// `RL_LAST_TXADDR(x)`.
    pub fn rl_last_txaddr(&self) -> u32 {
        u32::from(self.rl_cdata.last_tx.get()) * 4 + RL_TXADDR0
    }

    /// `RL_LAST_TXSTAT(x)`.
    pub fn rl_last_txstat(&self) -> u32 {
        u32::from(self.rl_cdata.last_tx.get()) * 4 + RL_TXSTAT0
    }

    /// `RL_LAST_TXMBUF(x)`.
    pub fn rl_last_txmbuf(&self) -> &Cell<Option<&'static Mbuf>> {
        &self.rl_cdata.rl_tx_chain[usize::from(self.rl_cdata.last_tx.get()) % 4]
    }

    /// `RL_LAST_TXMAP(x)`.
    pub fn rl_last_txmap(&self) -> &Cell<Option<&'static BusDmamap>> {
        &self.rl_cdata.rl_tx_dmamap[usize::from(self.rl_cdata.last_tx.get()) % 4]
    }
}

// SAFETY: `#[repr(C)]` with the device first; the arpcom, the MII data, the timeouts and the
// task are all-zero valid, and every other member is a `Cell` or an atomic of an integer, a
// pointer, a segment (integers) or an `Option`, or an array of such.
unsafe impl Softc for RlSoftc {}

/// `RL_TXTHRESH(x)`.
pub const fn rl_txthresh(x: u32) -> u32 {
    x << 11
}

/// `RL_IM_RXTIME(t)`.
pub const fn rl_im_rxtime(t: u32) -> u32 {
    t & 0xf
}

/// `RL_IM_TXTIME(t)`.
pub const fn rl_im_txtime(t: u32) -> u32 {
    (t & 0xf) << 8
}

/// `RL_TCPPKT(x)`.
pub const fn rl_tcppkt(x: u32) -> bool {
    (x & RL_RDESC_STAT_PROTOID) == RL_PROTOID_TCPIP
}

/// `RL_UDPPKT(x)`.
pub const fn rl_udppkt(x: u32) -> bool {
    (x & RL_RDESC_STAT_PROTOID) == RL_PROTOID_UDPIP
}

/// `RL_PKTSZ(x)`.
pub const fn rl_pktsz(x: u32) -> u32 {
    x // >> 3
}

/// `RL_NEXT_TXQ(sc, x)`.
pub const fn rl_next_txq(x: u32) -> u32 {
    (x + 1) % RL_TX_QLEN
}

/// `RL_ADDR_LO(y)`.
pub const fn rl_addr_lo(y: u64) -> u32 {
    (y & 0xFFFF_FFFF) as u32
}

/// `RL_ADDR_HI(y)`.
pub const fn rl_addr_hi(y: u64) -> u32 {
    (y >> 32) as u32
}

/// `RL_INC(x)`: `x = (x + 1) % RL_TX_LIST_CNT`.
pub fn rl_inc(x: &Cell<u8>) {
    x.set(((u32::from(x.get()) + 1) % RL_TX_LIST_CNT) as u8);
}

const _: () = assert!(size_of::<RlDesc>() == 16);
const _: () = assert!(size_of::<ReStats>() == 64);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        // Every simple define; the multi-line ones (RL_INTRS, RL_INTRS_CPLUS, RL_INTRS_TIMER,
        // RL_RDESC_STAT_ERRS), RL_RXBUFLEN (a `>>`) and those naming other headers are checked
        // below or in `macros`.
        let defs = crate::reftest::defines("sys/dev/ic/rtl81x9reg.h");
        crate::reftest::assert_defines!(defs; RL_IDR0, RL_IDR1, RL_IDR2, RL_IDR3, RL_IDR4, RL_IDR5, RL_MAR0, RL_MAR1, RL_MAR2, RL_MAR3, RL_MAR4, RL_MAR5, RL_MAR6, RL_MAR7, RL_TXSTAT0, RL_TXSTAT1, RL_TXSTAT2, RL_TXSTAT3, RL_TXADDR0, RL_TXADDR1, RL_TXADDR2, RL_TXADDR3, RL_RXADDR, RL_RX_EARLY_BYTES, RL_RX_EARLY_STAT, RL_COMMAND, RL_CURRXADDR, RL_CURRXBUF, RL_IMR, RL_ISR, RL_TXCFG, RL_RXCFG, RL_TIMERCNT, RL_MISSEDPKT, RL_EECMD, RL_8139_CFG0, RL_8139_CFG1, RL_8139_CFG3, RL_8139_CFG4, RL_8139_CFG5, RL_CFG0, RL_CFG1, RL_CFG2, RL_CFG3, RL_CFG4, RL_CFG5, RL_MEDIASTAT, RL_MII, RL_HALTCLK, RL_MULTIINTR, RL_PCIREV, RL_TXSTAT_ALL, RL_CSIDR, RL_CSIAR, RL_BMCR, RL_BMSR, RL_ANAR, RL_LPAR, RL_ANER, RL_DISCCNT, RL_FALSECAR, RL_NWAYTST, RL_RX_ER, RL_CSCFG, RL_DUMPSTATS_LO, RL_DUMPSTATS_HI, RL_TXLIST_ADDR_LO, RL_TXLIST_ADDR_HI, RL_TXLIST_ADDR_HPRIO_LO, RL_TXLIST_ADDR_HPRIO_HI, RL_TIMERINT, RL_TXSTART, RL_CPLUS_CMD, RL_RXLIST_ADDR_LO, RL_RXLIST_ADDR_HI, RL_EARLY_TX_THRESH, RL_GTXSTART, RL_TIMERINT_8169, RL_PHYAR, RL_TBICSR, RL_TBI_ANAR, RL_TBI_LPAR, RL_GMEDIASTAT, RL_MACDBG, RL_GPIO, RL_PMCH, RL_LDPS, RL_MAXRXPKTLEN, RL_IM, RL_MISC, RL_LEDSEL, RL_LED_LINK, RL_LED_ACT, RL_TXCFG_CLRABRT, RL_TXCFG_MAXDMA, RL_TXCFG_QUEUE_EMPTY, RL_TXCFG_CRCAPPEND, RL_TXCFG_LOOPBKTST, RL_TXCFG_IFG2, RL_TXCFG_IFG, RL_TXCFG_HWREV, RL_LOOPTEST_OFF, RL_LOOPTEST_ON, RL_LOOPTEST_ON_CPLUS, RL_HWREV_8169, RL_HWREV_8169S, RL_HWREV_8110S, RL_HWREV_8169_8110SB, RL_HWREV_8169_8110SCd, RL_HWREV_8401E, RL_HWREV_8102EL, RL_HWREV_8102EL_SPIN1, RL_HWREV_8168D, RL_HWREV_8168DP, RL_HWREV_8168E, RL_HWREV_8168E_VL, RL_HWREV_8168B_SPIN1, RL_HWREV_8100E, RL_HWREV_8101E, RL_HWREV_8102E, RL_HWREV_8103E, RL_HWREV_8168B_SPIN2, RL_HWREV_8168B_SPIN3, RL_HWREV_8100E_SPIN2, RL_HWREV_8168C, RL_HWREV_8168C_SPIN2, RL_HWREV_8168CP, RL_HWREV_8105E, RL_HWREV_8105E_SPIN1, RL_HWREV_8402, RL_HWREV_8106E, RL_HWREV_8168F, RL_HWREV_8411, RL_HWREV_8168G, RL_HWREV_8168EP, RL_HWREV_8168GU, RL_HWREV_8168H, RL_HWREV_8168FP, RL_HWREV_8411B, RL_HWREV_8139, RL_HWREV_8139A, RL_HWREV_8139AG, RL_HWREV_8139B, RL_HWREV_8130, RL_HWREV_8139C, RL_HWREV_8139D, RL_HWREV_8139CPLUS, RL_HWREV_8101, RL_HWREV_8100, RL_HWREV_8169_8110SBL, RL_HWREV_8169_8110SCe, RL_TXDMA_16BYTES, RL_TXDMA_32BYTES, RL_TXDMA_64BYTES, RL_TXDMA_128BYTES, RL_TXDMA_256BYTES, RL_TXDMA_512BYTES, RL_TXDMA_1024BYTES, RL_TXDMA_2048BYTES, RL_TXSTAT_LENMASK, RL_TXSTAT_OWN, RL_TXSTAT_TX_UNDERRUN, RL_TXSTAT_TX_OK, RL_TXSTAT_EARLY_THRESH, RL_TXSTAT_COLLCNT, RL_TXSTAT_CARR_HBEAT, RL_TXSTAT_OUTOFWIN, RL_TXSTAT_TXABRT, RL_TXSTAT_CARRLOSS, RL_ISR_RX_OK, RL_ISR_RX_ERR, RL_ISR_TX_OK, RL_ISR_TX_ERR, RL_ISR_RX_OVERRUN, RL_ISR_PKT_UNDERRUN, RL_ISR_LINKCHG, RL_ISR_FIFO_OFLOW, RL_ISR_TX_DESC_UNAVAIL, RL_ISR_SWI, RL_ISR_CABLE_LEN_CHGD, RL_ISR_PCS_TIMEOUT, RL_ISR_TIMEOUT_EXPIRED, RL_ISR_SYSTEM_ERR, RL_MEDIASTAT_RXPAUSE, RL_MEDIASTAT_TXPAUSE, RL_MEDIASTAT_LINK, RL_MEDIASTAT_SPEED10, RL_MEDIASTAT_RXFLOWCTL, RL_MEDIASTAT_TXFLOWCTL, RL_RXCFG_RX_ALLPHYS, RL_RXCFG_RX_INDIV, RL_RXCFG_RX_MULTI, RL_RXCFG_RX_BROAD, RL_RXCFG_RX_RUNT, RL_RXCFG_RX_ERRPKT, RL_RXCFG_WRAP, RL_RXCFG_EARLYOFFV2, RL_RXCFG_MAXDMA, RL_RXCFG_BURSZ, RL_RXCFG_EARLYOFF, RL_RXCFG_FIFOTHRESH, RL_RXCFG_EARLYTHRESH, RL_RXDMA_16BYTES, RL_RXDMA_32BYTES, RL_RXDMA_64BYTES, RL_RXDMA_128BYTES, RL_RXDMA_256BYTES, RL_RXDMA_512BYTES, RL_RXDMA_1024BYTES, RL_RXDMA_UNLIMITED, RL_RXBUF_8, RL_RXBUF_16, RL_RXBUF_32, RL_RXBUF_64, RL_RXFIFO_16BYTES, RL_RXFIFO_32BYTES, RL_RXFIFO_64BYTES, RL_RXFIFO_128BYTES, RL_RXFIFO_256BYTES, RL_RXFIFO_512BYTES, RL_RXFIFO_1024BYTES, RL_RXFIFO_NOTHRESH, RL_RXSTAT_RXOK, RL_RXSTAT_ALIGNERR, RL_RXSTAT_CRCERR, RL_RXSTAT_GIANT, RL_RXSTAT_RUNT, RL_RXSTAT_BADSYM, RL_RXSTAT_BROAD, RL_RXSTAT_INDIV, RL_RXSTAT_MULTI, RL_RXSTAT_LENMASK, RL_RXSTAT_UNFINISHED, RL_CMD_EMPTY_RXBUF, RL_CMD_TX_ENB, RL_CMD_RX_ENB, RL_CMD_RESET, RL_CMD_STOPREQ, RL_EE_DATAOUT, RL_EE_DATAIN, RL_EE_CLK, RL_EE_SEL, RL_EE_MODE, RL_EEMODE_OFF, RL_EEMODE_AUTOLOAD, RL_EEMODE_PROGRAM, RL_EEMODE_WRITECFG, RL_9346_ADDR_LEN, RL_9356_ADDR_LEN, RL_9346_WRITE, RL_9346_READ, RL_9346_ERASE, RL_9346_EWEN, RL_9346_EWEN_ADDR, RL_9456_EWDS, RL_9346_EWDS_ADDR, RL_EECMD_WRITE, RL_EECMD_READ, RL_EECMD_ERASE, RL_EECMD_LEN, RL_EEADDR_LEN0, RL_EEADDR_LEN1, RL_EECMD_READ_6BIT, RL_EECMD_READ_8BIT, RL_EE_ID, RL_EE_PCI_VID, RL_EE_PCI_DID, RL_EE_EADDR, RL_MII_CLK, RL_MII_DATAIN, RL_MII_DATAOUT, RL_MII_DIR, RL_CFG0_ROM0, RL_CFG0_ROM1, RL_CFG0_ROM2, RL_CFG0_PL0, RL_CFG0_PL1, RL_CFG0_10MBPS, RL_CFG0_PCS, RL_CFG0_SCR, RL_CFG1_PWRDWN, RL_CFG1_PME, RL_CFG1_SLEEP, RL_CFG1_VPDEN, RL_CFG1_IOMAP, RL_CFG1_MEMMAP, RL_CFG1_RSVD, RL_CFG1_LWACT, RL_CFG1_DRVLOAD, RL_CFG1_LED0, RL_CFG1_FULLDUPLEX, RL_CFG1_LED1, RL_CFG2_PCI_MASK, RL_CFG2_PCI_33MHZ, RL_CFG2_PCI_66MHZ, RL_CFG2_PCI_64BIT, RL_CFG2_AUXPWR, RL_CFG2_MSI, RL_CFG3_GRANTSEL, RL_CFG3_WOL_MAGIC, RL_CFG3_WOL_LINK, RL_CFG3_JUMBO_EN0, RL_CFG3_FAST_B2B, RL_CFG4_CUSTOM_LED, RL_CFG4_LWPTN, RL_CFG4_LWPME, RL_CFG4_JUMBO_EN1, RL_CFG4_8168E_JUMBO_EN1, RL_CFG5_WOL_BCAST, RL_CFG5_WOL_MCAST, RL_CFG5_WOL_UCAST, RL_CFG5_WOL_LANWAKE, RL_CFG5_PME_STS, RL_DUMPSTATS_START, RL_TXSTART_SWI, RL_TXSTART_START, RL_TXSTART_HPRIO_START, RL_CFG2_BUSFREQ, RL_CFG2_BUSWIDTH, RL_CFG2_AUXPWRSTS, RL_BUSFREQ_33MHZ, RL_BUSFREQ_66MHZ, RL_BUSWIDTH_32BITS, RL_BUSWIDTH_64BITS, RL_CPLUSCMD_TXENB, RL_CPLUSCMD_RXENB, RL_CPLUSCMD_PCI_MRW, RL_CPLUSCMD_PCI_DAC, RL_CPLUSCMD_RXCSUM_ENB, RL_CPLUSCMD_VLANSTRIP, RL_CPLUSCMD_MACSTAT_DIS, RL_CPLUSCMD_ASF, RL_CPLUSCMD_DBG_SEL, RL_CPLUSCMD_FORCE_TXFC, RL_CPLUSCMD_FORCE_RXFC, RL_CPLUSCMD_FORCE_HDPX, RL_CPLUSCMD_NORMAL_MODE, RL_CPLUSCMD_DBG_ENB, RL_CPLUSCMD_BIST_ENB, RL_EARLYTXTHRESH_CNT, RL_PHYAR_PHYDATA, RL_PHYAR_PHYREG, RL_PHYAR_BUSY, RL_GMEDIASTAT_FDX, RL_GMEDIASTAT_LINK, RL_GMEDIASTAT_10MBPS, RL_GMEDIASTAT_100MBPS, RL_GMEDIASTAT_1000MBPS, RL_GMEDIASTAT_RXFLOW, RL_GMEDIASTAT_TXFLOW, RL_GMEDIASTAT_TBI, RL_RX_BUF_SZ, RL_TX_LIST_CNT, RL_MIN_FRAMELEN, RL_TX_THRESH_INIT, RL_RX_FIFOTHRESH, RL_RX_MAXDMA, RL_TX_MAXDMA, RL_RXCFG_CONFIG, RL_TXCFG_CONFIG, RL_IM_MAGIC, RL_TDESC_CMD_FRAGLEN, RL_TDESC_CMD_TCPCSUM, RL_TDESC_CMD_UDPCSUM, RL_TDESC_CMD_IPCSUM, RL_TDESC_CMD_MSSVAL, RL_TDESC_CMD_LGSEND, RL_TDESC_CMD_EOF, RL_TDESC_CMD_SOF, RL_TDESC_CMD_EOR, RL_TDESC_CMD_OWN, RL_TDESC_VLANCTL_TAG, RL_TDESC_VLANCTL_DATA, RL_TDESC_CMD_IPCSUMV2, RL_TDESC_CMD_TCPCSUMV2, RL_TDESC_CMD_UDPCSUMV2, RL_TDESC_STAT_COLCNT, RL_TDESC_STAT_EXCESSCOL, RL_TDESC_STAT_LINKFAIL, RL_TDESC_STAT_OWINCOL, RL_TDESC_STAT_TXERRSUM, RL_TDESC_STAT_UNDERRUN, RL_TDESC_STAT_OWN, RL_RDESC_CMD_EOR, RL_RDESC_CMD_OWN, RL_RDESC_CMD_BUFLEN, RL_RDESC_STAT_OWN, RL_RDESC_STAT_EOR, RL_RDESC_STAT_SOF, RL_RDESC_STAT_EOF, RL_RDESC_STAT_FRALIGN, RL_RDESC_STAT_MCAST, RL_RDESC_STAT_UCAST, RL_RDESC_STAT_BCAST, RL_RDESC_STAT_BUFOFLOW, RL_RDESC_STAT_FIFOOFLOW, RL_RDESC_STAT_GIANT, RL_RDESC_STAT_RXERRSUM, RL_RDESC_STAT_RUNT, RL_RDESC_STAT_CRCERR, RL_RDESC_STAT_PROTOID, RL_RDESC_STAT_UDP, RL_RDESC_STAT_TCP, RL_RDESC_STAT_IPSUMBAD, RL_RDESC_STAT_UDPSUMBAD, RL_RDESC_STAT_TCPSUMBAD, RL_RDESC_STAT_FRAGLEN, RL_RDESC_STAT_GFRAGLEN, RL_RDESC_VLANCTL_DATA, RL_RDESC_IPV6, RL_RDESC_IPV4, RL_PROTOID_NONIP, RL_PROTOID_TCPIP, RL_PROTOID_UDPIP, RL_PROTOID_IP, RL_8169_TX_DESC_CNT, RL_8169_RX_DESC_CNT, RL_8139_TX_DESC_CNT, RL_8139_RX_DESC_CNT, RL_TX_DESC_CNT, RL_RX_DESC_CNT, RL_8169_NTXSEGS, RL_8139_NTXSEGS, RL_TX_QLEN, RL_RING_ALIGN, RL_JUMBO_FRAMELEN, MAX_NUM_MULTICAST_ADDRESSES, RL_MII_STARTDELIM, RL_MII_READOP, RL_MII_WRITEOP, RL_MII_TURNAROUND, RL_UNKNOWN, RL_8129, RL_8139, RL_FLAG_MSI, RL_FLAG_PCI64, RL_FLAG_PCIE, RL_FLAG_PHYWAKE, RL_FLAG_PAR, RL_FLAG_DESCV2, RL_FLAG_MACSTAT, RL_FLAG_HWIM, RL_FLAG_TIMERINTR, RL_FLAG_MACRESET, RL_FLAG_CMDSTOP, RL_FLAG_MACSLEEP, RL_FLAG_AUTOPAD, RL_FLAG_LINK, RL_FLAG_PHYWAKE_PM, RL_FLAG_EARLYOFF, RL_FLAG_EARLYOFFV2, RL_FLAG_RXDV_GATED, RL_FLAG_FASTETHER, RL_FLAG_CMDSTOP_WAIT_TXQ, RL_FLAG_JUMBOV2, RL_FLAG_WOL_MANLINK, RL_FLAG_WAIT_TXPOLL, RL_FLAG_WOLRXENB, RL_IMTYPE_NONE, RL_IMTYPE_SIM, RL_IMTYPE_HW, RL_IP4CSUMTX_MINLEN, RL_TIMEOUT, RL_PHY_TIMEOUT, RT_VENDORID, RT_DEVICEID_8129, RT_DEVICEID_8101E, RT_DEVICEID_8138, RT_DEVICEID_8139, RT_DEVICEID_8169SC, RT_DEVICEID_8168, RT_DEVICEID_8169, RT_DEVICEID_8100, ACCTON_VENDORID, ACCTON_DEVICEID_5030, DELTA_VENDORID, DELTA_DEVICEID_8139, ADDTRON_VENDORID, ADDTRON_DEVICEID_8139, DLINK_VENDORID, DLINK_DEVICEID_8139, DLINK_DEVICEID_8139_2, ABOCOM_DEVICEID_8139, RL_PCI_VENDOR_ID, RL_PCI_DEVICE_ID, RL_PCI_COMMAND, RL_PCI_STATUS, RL_PCI_CLASSCODE, RL_PCI_LATENCY_TIMER, RL_PCI_HEADER_TYPE, RL_PCI_LOIO, RL_PCI_LOMEM, RL_PCI_LOMEM64, RL_PCI_BIOSROM, RL_PCI_INTLINE, RL_PCI_INTPIN, RL_PCI_MINGNT, RL_PCI_MINLAT, RL_PCI_PMCSR, RL_PCI_RESETOPT, RL_PCI_EEPROM_DATA, RL_PCI_CAPID, RL_PCI_NEXTPTR, RL_PCI_PWRMGMTCAP, RL_PCI_PWRMGMTCTRL, RL_PSTATE_MASK, RL_PSTATE_D0, RL_PSTATE_D1, RL_PSTATE_D2, RL_PSTATE_D3, RL_PME_EN, RL_PME_STATUS);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn derived_values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/rtl81x9reg.h");
        // The values that name <netinet/if_ether.h> or <sys/mbuf.h>, with those spelled out.
        assert_eq!(defs["RL_MTU"], "ETHERMTU");
        assert_eq!(RL_MTU, 1500);
        assert_eq!(RL_IP4CSUMTX_PADLEN, 14 + 28);
        assert_eq!(RL_JUMBO_MTU_4K, 4 * 1024 - 14 - 4 - 4);
        assert_eq!(RL_JUMBO_MTU_6K, 6 * 1024 - 14 - 4 - 4);
        assert_eq!(RL_JUMBO_MTU_7K, 7 * 1024 - 14 - 4 - 4);
        assert_eq!(RL_JUMBO_MTU_9K, 9 * 1024 - 14 - 4 - 4);
        assert_eq!(RE_RX_DESC_BUFLEN, 2048 - RE_ETHER_ALIGN);
        assert_eq!(RL_INTRS, 0xc07f);
        assert_eq!(RL_INTRS_CPLUS, 0x805f);
        assert_eq!(RL_INTRS_TIMER, 0xc00a);
        assert_eq!(RL_RDESC_STAT_ERRS, 0x002c_0000);
    }

    #[test]
    fn macros() {
        assert_eq!(rl_txthresh(3), 3 << 11);
        assert_eq!(rl_im_rxtime(0x12) | rl_im_txtime(0x15), 0x0502);
        assert!(rl_tcppkt(RL_PROTOID_TCPIP | RL_RDESC_STAT_OWN));
        assert!(rl_udppkt(RL_PROTOID_UDPIP));
        assert!(!rl_udppkt(RL_PROTOID_IP));
        assert_eq!(rl_addr_lo(0x1_2345_6789), 0x2345_6789);
        assert_eq!(rl_addr_hi(0x1_2345_6789), 1);
        assert_eq!(rl_next_txq(RL_TX_QLEN - 1), 0);
        let x = Cell::new(3u8);
        rl_inc(&x);
        assert_eq!(x.get(), 0);
        assert_eq!(RL_RXBUFLEN, 1 << 16);
    }
}
/* </TESTS> */
