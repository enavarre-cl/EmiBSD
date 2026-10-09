/* <LICENSES> */
/*
 * Copyright (c) 1997, 1998, 1999
 *	Bill Paul <wpaul@ee.columbia.edu>.  All rights reserved.
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
 * $FreeBSD: src/sys/pci/if_dcreg.h,v 1.12 2000/10/05 17:36:14 wpaul Exp $
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/ic/dcreg.h>`: the DEC/Intel 21143 ("tulip") and clone register definitions, the
//! descriptor and list layouts, the SROM media blocks and `struct dc_softc` of dc(4).
//!
//! Upstream: sys/dev/ic/dcreg.h @ 3ce1f3f79392
//!
//! The C macros become methods of [`DcSoftc`]: `DC_IS_*` and `DC_HAS_BROKEN_RXSTATE` are
//! predicates, `CSR_READ_4`/`CSR_WRITE_4` and dc.c's `DC_SETBIT`/`DC_CLRBIT` (and the PNIC's
//! `DC_PN_GPIO_SETBIT`/`DC_PN_GPIO_CLRBIT`) are register accessors. The descriptor rings, the
//! setup frame buffer and the pad live in one `bus_dma(9)` allocation laid out as
//! `struct dc_list_data` ([`DcListData`]); the softc reaches its words through bounds-checked
//! volatile accessors.
//!
//! ## Deviations
//! - `SRM_MEDIA` (alpha only) is not defined, as on amd64 and arm64: `dc_srm_media` is left
//!   out, and the `#if 0` transmit threshold names are not compiled, as in C.
//! - The descriptor words are read and written one word at a time, volatile, through
//!   [`DcSoftc::rxd_read`] and friends, which take and return host order (the C's
//!   `htole32`/`letoh32` at each access).
//! - `struct dc_mediainfo` holds offsets into `dc_srom` where the C holds pointers into it
//!   (`dc_gp_ptr`, `dc_reset_ptr`), so its users index the SROM with bounds checks.
//! - `struct dc_swdesc`'s `sd_mbuf` is an `Option<&Mbuf>`; the C's stand-in for a setup frame
//!   (the address of `dc_sbuf` cast to an mbuf pointer) is the flag `sd_setup`.
//! - `struct dc_chain_data`'s indices and counts are `usize`.
//! - The union of `struct dc_eblock_sia` is a `#[repr(C)]` union of byte arrays.
//! - `dc_attach`, `dc_detach`, `dc_activate`, `dc_intr`, `dc_init`, `dc_stop`, `dc_reset`,
//!   `dc_eeprom_width`, `dc_read_srom` and `dc_parse_21143_srom`, which the header declares,
//!   are in `dc.rs`.

#![allow(non_upper_case_globals)] // the C's `DC_TYPE_987x5`, verbatim

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::offset_of;
use core::ptr::{self, NonNull};

use crate::dev::mii::miivar::MiiData;
use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BusDmaSegment, BusDmaTag, BusDmamap, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_read_4,
    bus_space_write_4,
};
use crate::netinet::if_ether::{Arpcom, ETHER_MIN_LEN};
use crate::sys::device::{Device, Softc};
use crate::sys::mbuf::Mbuf;
use crate::sys::timeout::Timeout;

/// `DC_BUSCTL`: bus control.
pub const DC_BUSCTL: u32 = 0x00;
/// `DC_TXSTART`: tx start demand.
pub const DC_TXSTART: u32 = 0x08;
/// `DC_RXSTART`: rx start demand.
pub const DC_RXSTART: u32 = 0x10;
/// `DC_RXADDR`: rx descriptor list start addr.
pub const DC_RXADDR: u32 = 0x18;
/// `DC_TXADDR`: tx descriptor list start addr.
pub const DC_TXADDR: u32 = 0x20;
/// `DC_ISR`: interrupt status register.
pub const DC_ISR: u32 = 0x28;
/// `DC_NETCFG`: network config register.
pub const DC_NETCFG: u32 = 0x30;
/// `DC_IMR`: interrupt mask.
pub const DC_IMR: u32 = 0x38;
/// `DC_FRAMESDISCARDED`: # of discarded frames.
pub const DC_FRAMESDISCARDED: u32 = 0x40;
/// `DC_SIO`: MII and ROM/EEPROM access.
pub const DC_SIO: u32 = 0x48;
/// `DC_ROM`: ROM programming address.
pub const DC_ROM: u32 = 0x50;
/// `DC_TIMER`: general timer.
pub const DC_TIMER: u32 = 0x58;
/// `DC_10BTSTAT`: SIA status.
pub const DC_10BTSTAT: u32 = 0x60;
/// `DC_SIARESET`: SIA connectivity.
pub const DC_SIARESET: u32 = 0x68;
/// `DC_10BTCTRL`: SIA transmit and receive.
pub const DC_10BTCTRL: u32 = 0x70;
/// `DC_WATCHDOG`: SIA and general purpose port.
pub const DC_WATCHDOG: u32 = 0x78;
/// `DC_SIAGP`: SIA and general purpose port (X3201).
pub const DC_SIAGP: u32 = 0x78;
/// `DC_TYPE_98713`.
pub const DC_TYPE_98713: u8 = 0x1;
/// `DC_TYPE_98713A`.
pub const DC_TYPE_98713A: u8 = 0x2;
/// `DC_TYPE_987x5`.
pub const DC_TYPE_987x5: u8 = 0x3;
/// `DC_TYPE_21143`: Intel 21143.
pub const DC_TYPE_21143: u8 = 0x4;
/// `DC_TYPE_ASIX`: ASIX AX88140A/AX88141.
pub const DC_TYPE_ASIX: u8 = 0x5;
/// `DC_TYPE_AL981`: ADMtek AL981 Comet.
pub const DC_TYPE_AL981: u8 = 0x6;
/// `DC_TYPE_AN983`: ADMtek AN983 Centaur.
pub const DC_TYPE_AN983: u8 = 0x7;
/// `DC_TYPE_DM9102`: Davicom DM9102.
pub const DC_TYPE_DM9102: u8 = 0x8;
/// `DC_TYPE_PNICII`: 82c115 PNIC II.
pub const DC_TYPE_PNICII: u8 = 0x9;
/// `DC_TYPE_PNIC`: 82c168/82c169 PNIC I.
pub const DC_TYPE_PNIC: u8 = 0xA;
/// `DC_TYPE_XIRCOM`: Xircom X3201.
pub const DC_TYPE_XIRCOM: u8 = 0xB;
/// `DC_TYPE_CONEXANT`: Conexant LANfinity RS7112.
pub const DC_TYPE_CONEXANT: u8 = 0xC;
/// `DC_TYPE_21145`: Intel 21145.
pub const DC_TYPE_21145: u8 = 0xD;
/// `DC_PMODE_MII`.
pub const DC_PMODE_MII: u8 = 0x1;
/// `DC_PMODE_SYM`.
pub const DC_PMODE_SYM: u8 = 0x2;
/// `DC_PMODE_SIA`.
pub const DC_PMODE_SIA: u8 = 0x3;
/// `DC_BUSCTL_RESET`.
pub const DC_BUSCTL_RESET: u32 = 0x00000001;
/// `DC_BUSCTL_ARBITRATION`.
pub const DC_BUSCTL_ARBITRATION: u32 = 0x00000002;
/// `DC_BUSCTL_SKIPLEN`.
pub const DC_BUSCTL_SKIPLEN: u32 = 0x0000007C;
/// `DC_BUSCTL_BUF_BIGENDIAN`.
pub const DC_BUSCTL_BUF_BIGENDIAN: u32 = 0x00000080;
/// `DC_BUSCTL_BURSTLEN`.
pub const DC_BUSCTL_BURSTLEN: u32 = 0x00003F00;
/// `DC_BUSCTL_CACHEALIGN`.
pub const DC_BUSCTL_CACHEALIGN: u32 = 0x0000C000;
/// `DC_BUSCTL_TXPOLL`.
pub const DC_BUSCTL_TXPOLL: u32 = 0x000E0000;
/// `DC_BUSCTL_DBO`.
pub const DC_BUSCTL_DBO: u32 = 0x00100000;
/// `DC_BUSCTL_MRME`.
pub const DC_BUSCTL_MRME: u32 = 0x00200000;
/// `DC_BUSCTL_MRLE`.
pub const DC_BUSCTL_MRLE: u32 = 0x00800000;
/// `DC_BUSCTL_MWIE`.
pub const DC_BUSCTL_MWIE: u32 = 0x01000000;
/// `DC_BUSCTL_ONNOW_ENB`.
pub const DC_BUSCTL_ONNOW_ENB: u32 = 0x04000000;
/// `DC_SKIPLEN_1LONG`.
pub const DC_SKIPLEN_1LONG: u32 = 0x00000004;
/// `DC_SKIPLEN_2LONG`.
pub const DC_SKIPLEN_2LONG: u32 = 0x00000008;
/// `DC_SKIPLEN_3LONG`.
pub const DC_SKIPLEN_3LONG: u32 = 0x00000010;
/// `DC_SKIPLEN_4LONG`.
pub const DC_SKIPLEN_4LONG: u32 = 0x00000020;
/// `DC_SKIPLEN_5LONG`.
pub const DC_SKIPLEN_5LONG: u32 = 0x00000040;
/// `DC_CACHEALIGN_NONE`.
pub const DC_CACHEALIGN_NONE: u32 = 0x00000000;
/// `DC_CACHEALIGN_8LONG`.
pub const DC_CACHEALIGN_8LONG: u32 = 0x00004000;
/// `DC_CACHEALIGN_16LONG`.
pub const DC_CACHEALIGN_16LONG: u32 = 0x00008000;
/// `DC_CACHEALIGN_32LONG`.
pub const DC_CACHEALIGN_32LONG: u32 = 0x0000C000;
/// `DC_BURSTLEN_USECA`.
pub const DC_BURSTLEN_USECA: u32 = 0x00000000;
/// `DC_BURSTLEN_1LONG`.
pub const DC_BURSTLEN_1LONG: u32 = 0x00000100;
/// `DC_BURSTLEN_2LONG`.
pub const DC_BURSTLEN_2LONG: u32 = 0x00000200;
/// `DC_BURSTLEN_4LONG`.
pub const DC_BURSTLEN_4LONG: u32 = 0x00000400;
/// `DC_BURSTLEN_8LONG`.
pub const DC_BURSTLEN_8LONG: u32 = 0x00000800;
/// `DC_BURSTLEN_16LONG`.
pub const DC_BURSTLEN_16LONG: u32 = 0x00001000;
/// `DC_BURSTLEN_32LONG`.
pub const DC_BURSTLEN_32LONG: u32 = 0x00002000;
/// `DC_TXPOLL_OFF`.
pub const DC_TXPOLL_OFF: u32 = 0x00000000;
/// `DC_TXPOLL_1`.
pub const DC_TXPOLL_1: u32 = 0x00020000;
/// `DC_TXPOLL_2`.
pub const DC_TXPOLL_2: u32 = 0x00040000;
/// `DC_TXPOLL_3`.
pub const DC_TXPOLL_3: u32 = 0x00060000;
/// `DC_TXPOLL_4`.
pub const DC_TXPOLL_4: u32 = 0x00080000;
/// `DC_TXPOLL_5`.
pub const DC_TXPOLL_5: u32 = 0x000A0000;
/// `DC_TXPOLL_6`.
pub const DC_TXPOLL_6: u32 = 0x000C0000;
/// `DC_TXPOLL_7`.
pub const DC_TXPOLL_7: u32 = 0x000E0000;
/// `DC_ISR_TX_OK`.
pub const DC_ISR_TX_OK: u32 = 0x00000001;
/// `DC_ISR_TX_IDLE`.
pub const DC_ISR_TX_IDLE: u32 = 0x00000002;
/// `DC_ISR_TX_NOBUF`.
pub const DC_ISR_TX_NOBUF: u32 = 0x00000004;
/// `DC_ISR_TX_JABBERTIMEO`.
pub const DC_ISR_TX_JABBERTIMEO: u32 = 0x00000008;
/// `DC_ISR_LINKGOOD`.
pub const DC_ISR_LINKGOOD: u32 = 0x00000010;
/// `DC_ISR_TX_UNDERRUN`.
pub const DC_ISR_TX_UNDERRUN: u32 = 0x00000020;
/// `DC_ISR_RX_OK`.
pub const DC_ISR_RX_OK: u32 = 0x00000040;
/// `DC_ISR_RX_NOBUF`.
pub const DC_ISR_RX_NOBUF: u32 = 0x00000080;
/// `DC_ISR_RX_READ`.
pub const DC_ISR_RX_READ: u32 = 0x00000100;
/// `DC_ISR_RX_WATDOGTIMEO`.
pub const DC_ISR_RX_WATDOGTIMEO: u32 = 0x00000200;
/// `DC_ISR_TX_EARLY`.
pub const DC_ISR_TX_EARLY: u32 = 0x00000400;
/// `DC_ISR_TIMER_EXPIRED`.
pub const DC_ISR_TIMER_EXPIRED: u32 = 0x00000800;
/// `DC_ISR_LINKFAIL`.
pub const DC_ISR_LINKFAIL: u32 = 0x00001000;
/// `DC_ISR_BUS_ERR`.
pub const DC_ISR_BUS_ERR: u32 = 0x00002000;
/// `DC_ISR_RX_EARLY`.
pub const DC_ISR_RX_EARLY: u32 = 0x00004000;
/// `DC_ISR_ABNORMAL`.
pub const DC_ISR_ABNORMAL: u32 = 0x00008000;
/// `DC_ISR_NORMAL`.
pub const DC_ISR_NORMAL: u32 = 0x00010000;
/// `DC_ISR_RX_STATE`.
pub const DC_ISR_RX_STATE: u32 = 0x000E0000;
/// `DC_ISR_TX_STATE`.
pub const DC_ISR_TX_STATE: u32 = 0x00700000;
/// `DC_ISR_BUSERRTYPE`.
pub const DC_ISR_BUSERRTYPE: u32 = 0x03800000;
/// `DC_ISR_100MBPSLINK`.
pub const DC_ISR_100MBPSLINK: u32 = 0x08000000;
/// `DC_ISR_MAGICKPACK`.
pub const DC_ISR_MAGICKPACK: u32 = 0x10000000;
/// `DC_RXSTATE_STOPPED`: 000 - Stopped.
pub const DC_RXSTATE_STOPPED: u32 = 0x00000000;
/// `DC_RXSTATE_FETCH`: 001 - Fetching descriptor.
pub const DC_RXSTATE_FETCH: u32 = 0x00020000;
/// `DC_RXSTATE_ENDCHECK`: 010 - check for rx end.
pub const DC_RXSTATE_ENDCHECK: u32 = 0x00040000;
/// `DC_RXSTATE_WAIT`: 011 - waiting for packet.
pub const DC_RXSTATE_WAIT: u32 = 0x00060000;
/// `DC_RXSTATE_SUSPEND`: 100 - suspend rx.
pub const DC_RXSTATE_SUSPEND: u32 = 0x00080000;
/// `DC_RXSTATE_CLOSE`: 101 - close tx desc.
pub const DC_RXSTATE_CLOSE: u32 = 0x000A0000;
/// `DC_RXSTATE_FLUSH`: 110 - flush from FIFO.
pub const DC_RXSTATE_FLUSH: u32 = 0x000C0000;
/// `DC_RXSTATE_DEQUEUE`: 111 - dequeue from FIFO.
pub const DC_RXSTATE_DEQUEUE: u32 = 0x000E0000;
/// `DC_TXSTATE_RESET`: 000 - reset.
pub const DC_TXSTATE_RESET: u32 = 0x00000000;
/// `DC_TXSTATE_FETCH`: 001 - fetching descriptor.
pub const DC_TXSTATE_FETCH: u32 = 0x00100000;
/// `DC_TXSTATE_WAITEND`: 010 - wait for tx end.
pub const DC_TXSTATE_WAITEND: u32 = 0x00200000;
/// `DC_TXSTATE_READING`: 011 - read and enqueue.
pub const DC_TXSTATE_READING: u32 = 0x00300000;
/// `DC_TXSTATE_RSVD`: 100 - reserved.
pub const DC_TXSTATE_RSVD: u32 = 0x00400000;
/// `DC_TXSTATE_SETUP`: 101 - setup packet.
pub const DC_TXSTATE_SETUP: u32 = 0x00500000;
/// `DC_TXSTATE_SUSPEND`: 110 - suspend tx.
pub const DC_TXSTATE_SUSPEND: u32 = 0x00600000;
/// `DC_TXSTATE_CLOSE`: 111 - close tx desc.
pub const DC_TXSTATE_CLOSE: u32 = 0x00700000;
/// `DC_NETCFG_RX_HASHPERF`.
pub const DC_NETCFG_RX_HASHPERF: u32 = 0x00000001;
/// `DC_NETCFG_RX_ON`.
pub const DC_NETCFG_RX_ON: u32 = 0x00000002;
/// `DC_NETCFG_RX_HASHONLY`.
pub const DC_NETCFG_RX_HASHONLY: u32 = 0x00000004;
/// `DC_NETCFG_RX_BADFRAMES`.
pub const DC_NETCFG_RX_BADFRAMES: u32 = 0x00000008;
/// `DC_NETCFG_RX_INVFILT`.
pub const DC_NETCFG_RX_INVFILT: u32 = 0x00000010;
/// `DC_NETCFG_BACKOFFCNT`.
pub const DC_NETCFG_BACKOFFCNT: u32 = 0x00000020;
/// `DC_NETCFG_RX_PROMISC`.
pub const DC_NETCFG_RX_PROMISC: u32 = 0x00000040;
/// `DC_NETCFG_RX_ALLMULTI`.
pub const DC_NETCFG_RX_ALLMULTI: u32 = 0x00000080;
/// `DC_NETCFG_FULLDUPLEX`.
pub const DC_NETCFG_FULLDUPLEX: u32 = 0x00000200;
/// `DC_NETCFG_LOOPBACK`.
pub const DC_NETCFG_LOOPBACK: u32 = 0x00000C00;
/// `DC_NETCFG_FORCECOLL`.
pub const DC_NETCFG_FORCECOLL: u32 = 0x00001000;
/// `DC_NETCFG_TX_ON`.
pub const DC_NETCFG_TX_ON: u32 = 0x00002000;
/// `DC_NETCFG_TX_THRESH`.
pub const DC_NETCFG_TX_THRESH: u32 = 0x0000C000;
/// `DC_NETCFG_TX_BACKOFF`.
pub const DC_NETCFG_TX_BACKOFF: u32 = 0x00020000;
/// `DC_NETCFG_PORTSEL`: 0 == 10, 1 == 100.
pub const DC_NETCFG_PORTSEL: u32 = 0x00040000;
/// `DC_NETCFG_HEARTBEAT`.
pub const DC_NETCFG_HEARTBEAT: u32 = 0x00080000;
/// `DC_NETCFG_STORENFWD`.
pub const DC_NETCFG_STORENFWD: u32 = 0x00200000;
/// `DC_NETCFG_SPEEDSEL`: 1 == 10, 0 == 100.
pub const DC_NETCFG_SPEEDSEL: u32 = 0x00400000;
/// `DC_NETCFG_PCS`.
pub const DC_NETCFG_PCS: u32 = 0x00800000;
/// `DC_NETCFG_SCRAMBLER`.
pub const DC_NETCFG_SCRAMBLER: u32 = 0x01000000;
/// `DC_NETCFG_NO_RXCRC`.
pub const DC_NETCFG_NO_RXCRC: u32 = 0x02000000;
/// `DC_NETCFG_RX_ALL`.
pub const DC_NETCFG_RX_ALL: u32 = 0x40000000;
/// `DC_NETCFG_CAPEFFECT`.
pub const DC_NETCFG_CAPEFFECT: u32 = 0x80000000;
/// `DC_OPMODE_NORM`.
pub const DC_OPMODE_NORM: u32 = 0x00000000;
/// `DC_OPMODE_INTLOOP`.
pub const DC_OPMODE_INTLOOP: u32 = 0x00000400;
/// `DC_OPMODE_EXTLOOP`.
pub const DC_OPMODE_EXTLOOP: u32 = 0x00000800;
/// `DC_TXTHRESH_MIN`.
pub const DC_TXTHRESH_MIN: u32 = 0x00000000;
/// `DC_TXTHRESH_INC`.
pub const DC_TXTHRESH_INC: u32 = 0x00004000;
/// `DC_TXTHRESH_MAX`.
pub const DC_TXTHRESH_MAX: u32 = 0x0000C000;
/// `DC_IMR_TX_OK`.
pub const DC_IMR_TX_OK: u32 = 0x00000001;
/// `DC_IMR_TX_IDLE`.
pub const DC_IMR_TX_IDLE: u32 = 0x00000002;
/// `DC_IMR_TX_NOBUF`.
pub const DC_IMR_TX_NOBUF: u32 = 0x00000004;
/// `DC_IMR_TX_JABBERTIMEO`.
pub const DC_IMR_TX_JABBERTIMEO: u32 = 0x00000008;
/// `DC_IMR_LINKGOOD`.
pub const DC_IMR_LINKGOOD: u32 = 0x00000010;
/// `DC_IMR_TX_UNDERRUN`.
pub const DC_IMR_TX_UNDERRUN: u32 = 0x00000020;
/// `DC_IMR_RX_OK`.
pub const DC_IMR_RX_OK: u32 = 0x00000040;
/// `DC_IMR_RX_NOBUF`.
pub const DC_IMR_RX_NOBUF: u32 = 0x00000080;
/// `DC_IMR_RX_READ`.
pub const DC_IMR_RX_READ: u32 = 0x00000100;
/// `DC_IMR_RX_WATDOGTIMEO`.
pub const DC_IMR_RX_WATDOGTIMEO: u32 = 0x00000200;
/// `DC_IMR_TX_EARLY`.
pub const DC_IMR_TX_EARLY: u32 = 0x00000400;
/// `DC_IMR_TIMER_EXPIRED`.
pub const DC_IMR_TIMER_EXPIRED: u32 = 0x00000800;
/// `DC_IMR_LINKFAIL`.
pub const DC_IMR_LINKFAIL: u32 = 0x00001000;
/// `DC_IMR_BUS_ERR`.
pub const DC_IMR_BUS_ERR: u32 = 0x00002000;
/// `DC_IMR_RX_EARLY`.
pub const DC_IMR_RX_EARLY: u32 = 0x00004000;
/// `DC_IMR_ABNORMAL`.
pub const DC_IMR_ABNORMAL: u32 = 0x00008000;
/// `DC_IMR_NORMAL`.
pub const DC_IMR_NORMAL: u32 = 0x00010000;
/// `DC_IMR_100MBPSLINK`.
pub const DC_IMR_100MBPSLINK: u32 = 0x08000000;
/// `DC_IMR_MAGICKPACK`.
pub const DC_IMR_MAGICKPACK: u32 = 0x10000000;
/// `DC_SIO_EE_CS`: EEPROM chip select.
pub const DC_SIO_EE_CS: u32 = 0x00000001;
/// `DC_SIO_EE_CLK`: EEPROM clock.
pub const DC_SIO_EE_CLK: u32 = 0x00000002;
/// `DC_SIO_EE_DATAIN`: EEPROM data output.
pub const DC_SIO_EE_DATAIN: u32 = 0x00000004;
/// `DC_SIO_EE_DATAOUT`: EEPROM data input.
pub const DC_SIO_EE_DATAOUT: u32 = 0x00000008;
/// `DC_SIO_ROMDATA4`.
pub const DC_SIO_ROMDATA4: u32 = 0x00000010;
/// `DC_SIO_ROMDATA5`.
pub const DC_SIO_ROMDATA5: u32 = 0x00000020;
/// `DC_SIO_ROMDATA6`.
pub const DC_SIO_ROMDATA6: u32 = 0x00000040;
/// `DC_SIO_ROMDATA7`.
pub const DC_SIO_ROMDATA7: u32 = 0x00000080;
/// `DC_SIO_EESEL`.
pub const DC_SIO_EESEL: u32 = 0x00000800;
/// `DC_SIO_ROMSEL`.
pub const DC_SIO_ROMSEL: u32 = 0x00001000;
/// `DC_SIO_ROMCTL_WRITE`.
pub const DC_SIO_ROMCTL_WRITE: u32 = 0x00002000;
/// `DC_SIO_ROMCTL_READ`.
pub const DC_SIO_ROMCTL_READ: u32 = 0x00004000;
/// `DC_SIO_MII_CLK`: MDIO clock.
pub const DC_SIO_MII_CLK: u32 = 0x00010000;
/// `DC_SIO_MII_DATAOUT`: MDIO data out.
pub const DC_SIO_MII_DATAOUT: u32 = 0x00020000;
/// `DC_SIO_MII_DIR`: MDIO dir.
pub const DC_SIO_MII_DIR: u32 = 0x00040000;
/// `DC_SIO_MII_DATAIN`: MDIO data in.
pub const DC_SIO_MII_DATAIN: u32 = 0x00080000;
/// `DC_EECMD_WRITE`.
pub const DC_EECMD_WRITE: i32 = 0x140;
/// `DC_EECMD_READ`.
pub const DC_EECMD_READ: i32 = 0x180;
/// `DC_EECMD_ERASE`.
pub const DC_EECMD_ERASE: i32 = 0x1c0;
/// `DC_EE_NODEADDR_OFFSET`.
pub const DC_EE_NODEADDR_OFFSET: i32 = 0x70;
/// `DC_EE_NODEADDR`.
pub const DC_EE_NODEADDR: i32 = 10;
/// `DC_TIMER_CLKDIV`: clock/16.
pub const DC_TIMER_CLKDIV: u32 = 0x80000000;
/// `DC_TIMER_TXTIMER`: TX intr delay timer.
pub const DC_TIMER_TXTIMER: u32 = 0x78000000;
/// `DC_TIMER_TXCOUNT`: TX intr delay counter.
pub const DC_TIMER_TXCOUNT: u32 = 0x07000000;
/// `DC_TIMER_RXTIMER`: RX intr delay timer.
pub const DC_TIMER_RXTIMER: u32 = 0x00F00000;
/// `DC_TIMER_RXCOUNT`: RX intr delay counter.
pub const DC_TIMER_RXCOUNT: u32 = 0x000E0000;
/// `DC_TIMER_CONTINUOUS`.
pub const DC_TIMER_CONTINUOUS: u32 = 0x00010000;
/// `DC_TIMER_VALUE`: 81.92us.
pub const DC_TIMER_VALUE: u32 = 0x0000FFFF;
/// `DC_TSTAT_MIIACT`: MII port activity.
pub const DC_TSTAT_MIIACT: u32 = 0x00000001;
/// `DC_TSTAT_LS100`: link status of 100baseTX.
pub const DC_TSTAT_LS100: u32 = 0x00000002;
/// `DC_TSTAT_LS10`: link status of 10baseT.
pub const DC_TSTAT_LS10: u32 = 0x00000004;
/// `DC_TSTAT_AUTOPOLARITY`.
pub const DC_TSTAT_AUTOPOLARITY: u32 = 0x00000008;
/// `DC_TSTAT_AUIACT`: AUI activity.
pub const DC_TSTAT_AUIACT: u32 = 0x00000100;
/// `DC_TSTAT_10BTACT`: 10baseT activity.
pub const DC_TSTAT_10BTACT: u32 = 0x00000200;
/// `DC_TSTAT_NSN`: non-stable FLPs detected.
pub const DC_TSTAT_NSN: u32 = 0x00000400;
/// `DC_TSTAT_REMFAULT`.
pub const DC_TSTAT_REMFAULT: u32 = 0x00000800;
/// `DC_TSTAT_ANEGSTAT`.
pub const DC_TSTAT_ANEGSTAT: u32 = 0x00007000;
/// `DC_TSTAT_LP_CAN_NWAY`: link partner supports NWAY.
pub const DC_TSTAT_LP_CAN_NWAY: u32 = 0x00008000;
/// `DC_TSTAT_LPCODEWORD`: link partner's code word.
pub const DC_TSTAT_LPCODEWORD: u32 = 0xFFFF0000;
/// `DC_ASTAT_DISABLE`.
pub const DC_ASTAT_DISABLE: u32 = 0x00000000;
/// `DC_ASTAT_TXDISABLE`.
pub const DC_ASTAT_TXDISABLE: u32 = 0x00001000;
/// `DC_ASTAT_ABDETECT`.
pub const DC_ASTAT_ABDETECT: u32 = 0x00002000;
/// `DC_ASTAT_ACKDETECT`.
pub const DC_ASTAT_ACKDETECT: u32 = 0x00003000;
/// `DC_ASTAT_CMPACKDETECT`.
pub const DC_ASTAT_CMPACKDETECT: u32 = 0x00004000;
/// `DC_ASTAT_AUTONEGCMP`.
pub const DC_ASTAT_AUTONEGCMP: u32 = 0x00005000;
/// `DC_ASTAT_LINKCHECK`.
pub const DC_ASTAT_LINKCHECK: u32 = 0x00006000;
/// `DC_SIA_RESET`.
pub const DC_SIA_RESET: u32 = 0x00000001;
/// `DC_SIA_AUI`: AUI or 10baseT.
pub const DC_SIA_AUI: u32 = 0x00000008;
/// `DC_TCTL_ENCODER_ENB`.
pub const DC_TCTL_ENCODER_ENB: u32 = 0x00000001;
/// `DC_TCTL_LOOPBACK`.
pub const DC_TCTL_LOOPBACK: u32 = 0x00000002;
/// `DC_TCTL_DRIVER_ENB`.
pub const DC_TCTL_DRIVER_ENB: u32 = 0x00000004;
/// `DC_TCTL_LNKPULSE_ENB`.
pub const DC_TCTL_LNKPULSE_ENB: u32 = 0x00000008;
/// `DC_TCTL_HALFDUPLEX`.
pub const DC_TCTL_HALFDUPLEX: u32 = 0x00000040;
/// `DC_TCTL_AUTONEGENBL`.
pub const DC_TCTL_AUTONEGENBL: u32 = 0x00000080;
/// `DC_TCTL_RX_SQUELCH`.
pub const DC_TCTL_RX_SQUELCH: u32 = 0x00000100;
/// `DC_TCTL_COLL_SQUELCH`.
pub const DC_TCTL_COLL_SQUELCH: u32 = 0x00000200;
/// `DC_TCTL_COLL_DETECT`.
pub const DC_TCTL_COLL_DETECT: u32 = 0x00000400;
/// `DC_TCTL_SQE_ENB`.
pub const DC_TCTL_SQE_ENB: u32 = 0x00000800;
/// `DC_TCTL_LINKTEST`.
pub const DC_TCTL_LINKTEST: u32 = 0x00001000;
/// `DC_TCTL_AUTOPOLARITY`.
pub const DC_TCTL_AUTOPOLARITY: u32 = 0x00002000;
/// `DC_TCTL_SET_POL_PLUS`.
pub const DC_TCTL_SET_POL_PLUS: u32 = 0x00004000;
/// `DC_TCTL_AUTOSENSE`: 10bt/AUI autosense.
pub const DC_TCTL_AUTOSENSE: u32 = 0x00008000;
/// `DC_TCTL_100BTXHALF`.
pub const DC_TCTL_100BTXHALF: u32 = 0x00010000;
/// `DC_TCTL_100BTXFULL`.
pub const DC_TCTL_100BTXFULL: u32 = 0x00020000;
/// `DC_TCTL_100BT4`.
pub const DC_TCTL_100BT4: u32 = 0x00040000;
/// `DC_WDOG_JABBERDIS`.
pub const DC_WDOG_JABBERDIS: u32 = 0x00000001;
/// `DC_WDOG_HOSTUNJAB`.
pub const DC_WDOG_HOSTUNJAB: u32 = 0x00000002;
/// `DC_WDOG_JABBERCLK`.
pub const DC_WDOG_JABBERCLK: u32 = 0x00000004;
/// `DC_WDOG_RXWDOGDIS`.
pub const DC_WDOG_RXWDOGDIS: u32 = 0x00000010;
/// `DC_WDOG_RXWDOGCLK`.
pub const DC_WDOG_RXWDOGCLK: u32 = 0x00000020;
/// `DC_WDOG_MUSTBEZERO`.
pub const DC_WDOG_MUSTBEZERO: u32 = 0x00000100;
/// `DC_WDOG_AUIBNC`.
pub const DC_WDOG_AUIBNC: u32 = 0x00100000;
/// `DC_WDOG_ACTIVITY`.
pub const DC_WDOG_ACTIVITY: u32 = 0x00200000;
/// `DC_WDOG_LINK`.
pub const DC_WDOG_LINK: u32 = 0x00800000;
/// `DC_WDOG_CTLWREN`.
pub const DC_WDOG_CTLWREN: u32 = 0x08000000;
/// `DC_SIAGP_RXMATCH`.
pub const DC_SIAGP_RXMATCH: u32 = 0x40000000;
/// `DC_SIAGP_INT1`.
pub const DC_SIAGP_INT1: u32 = 0x20000000;
/// `DC_SIAGP_INT0`.
pub const DC_SIAGP_INT0: u32 = 0x10000000;
/// `DC_SIAGP_WRITE_EN`.
pub const DC_SIAGP_WRITE_EN: u32 = 0x08000000;
/// `DC_SIAGP_RXMATCH_EN`.
pub const DC_SIAGP_RXMATCH_EN: u32 = 0x04000000;
/// `DC_SIAGP_INT1_EN`.
pub const DC_SIAGP_INT1_EN: u32 = 0x02000000;
/// `DC_SIAGP_INT0_EN`.
pub const DC_SIAGP_INT0_EN: u32 = 0x01000000;
/// `DC_SIAGP_LED3`.
pub const DC_SIAGP_LED3: u32 = 0x00800000;
/// `DC_SIAGP_LED2`.
pub const DC_SIAGP_LED2: u32 = 0x00400000;
/// `DC_SIAGP_LED1`.
pub const DC_SIAGP_LED1: u32 = 0x00200000;
/// `DC_SIAGP_LED0`.
pub const DC_SIAGP_LED0: u32 = 0x00100000;
/// `DC_SIAGP_MD_GP3_OUTPUT`.
pub const DC_SIAGP_MD_GP3_OUTPUT: u32 = 0x00080000;
/// `DC_SIAGP_MD_GP2_OUTPUT`.
pub const DC_SIAGP_MD_GP2_OUTPUT: u32 = 0x00040000;
/// `DC_SIAGP_MD_GP1_OUTPUT`.
pub const DC_SIAGP_MD_GP1_OUTPUT: u32 = 0x00020000;
/// `DC_SIAGP_MD_GP0_OUTPUT`.
pub const DC_SIAGP_MD_GP0_OUTPUT: u32 = 0x00010000;
/// `DC_SFRAME_LEN`.
pub const DC_SFRAME_LEN: usize = 192;
/// `DC_RXSTAT_FIFOOFLOW`.
pub const DC_RXSTAT_FIFOOFLOW: u32 = 0x00000001;
/// `DC_RXSTAT_CRCERR`.
pub const DC_RXSTAT_CRCERR: u32 = 0x00000002;
/// `DC_RXSTAT_DRIBBLE`.
pub const DC_RXSTAT_DRIBBLE: u32 = 0x00000004;
/// `DC_RXSTAT_MIIERE`.
pub const DC_RXSTAT_MIIERE: u32 = 0x00000008;
/// `DC_RXSTAT_WATCHDOG`.
pub const DC_RXSTAT_WATCHDOG: u32 = 0x00000010;
/// `DC_RXSTAT_FRAMETYPE`: 0 == IEEE 802.3.
pub const DC_RXSTAT_FRAMETYPE: u32 = 0x00000020;
/// `DC_RXSTAT_COLLSEEN`.
pub const DC_RXSTAT_COLLSEEN: u32 = 0x00000040;
/// `DC_RXSTAT_GIANT`.
pub const DC_RXSTAT_GIANT: u32 = 0x00000080;
/// `DC_RXSTAT_LASTFRAG`.
pub const DC_RXSTAT_LASTFRAG: u32 = 0x00000100;
/// `DC_RXSTAT_FIRSTFRAG`.
pub const DC_RXSTAT_FIRSTFRAG: u32 = 0x00000200;
/// `DC_RXSTAT_MULTICAST`.
pub const DC_RXSTAT_MULTICAST: u32 = 0x00000400;
/// `DC_RXSTAT_RUNT`.
pub const DC_RXSTAT_RUNT: u32 = 0x00000800;
/// `DC_RXSTAT_RXTYPE`.
pub const DC_RXSTAT_RXTYPE: u32 = 0x00003000;
/// `DC_RXSTAT_DE`.
pub const DC_RXSTAT_DE: u32 = 0x00004000;
/// `DC_RXSTAT_RXERR`.
pub const DC_RXSTAT_RXERR: u32 = 0x00008000;
/// `DC_RXSTAT_RXLEN`.
pub const DC_RXSTAT_RXLEN: u32 = 0x3FFF0000;
/// `DC_RXSTAT_OWN`.
pub const DC_RXSTAT_OWN: u32 = 0x80000000;
/// `DC_RXCTL_BUFLEN1`.
pub const DC_RXCTL_BUFLEN1: u32 = 0x00000FFF;
/// `DC_RXCTL_BUFLEN2`.
pub const DC_RXCTL_BUFLEN2: u32 = 0x00FFF000;
/// `DC_RXCTL_RLINK`.
pub const DC_RXCTL_RLINK: u32 = 0x01000000;
/// `DC_RXCTL_RLAST`.
pub const DC_RXCTL_RLAST: u32 = 0x02000000;
/// `DC_TXSTAT_DEFER`.
pub const DC_TXSTAT_DEFER: u32 = 0x00000001;
/// `DC_TXSTAT_UNDERRUN`.
pub const DC_TXSTAT_UNDERRUN: u32 = 0x00000002;
/// `DC_TXSTAT_LINKFAIL`.
pub const DC_TXSTAT_LINKFAIL: u32 = 0x00000003;
/// `DC_TXSTAT_COLLCNT`.
pub const DC_TXSTAT_COLLCNT: u32 = 0x00000078;
/// `DC_TXSTAT_SQE`.
pub const DC_TXSTAT_SQE: u32 = 0x00000080;
/// `DC_TXSTAT_EXCESSCOLL`.
pub const DC_TXSTAT_EXCESSCOLL: u32 = 0x00000100;
/// `DC_TXSTAT_LATECOLL`.
pub const DC_TXSTAT_LATECOLL: u32 = 0x00000200;
/// `DC_TXSTAT_NOCARRIER`.
pub const DC_TXSTAT_NOCARRIER: u32 = 0x00000400;
/// `DC_TXSTAT_CARRLOST`.
pub const DC_TXSTAT_CARRLOST: u32 = 0x00000800;
/// `DC_TXSTAT_JABTIMEO`.
pub const DC_TXSTAT_JABTIMEO: u32 = 0x00004000;
/// `DC_TXSTAT_ERRSUM`.
pub const DC_TXSTAT_ERRSUM: u32 = 0x00008000;
/// `DC_TXSTAT_OWN`.
pub const DC_TXSTAT_OWN: u32 = 0x80000000;
/// `DC_TXCTL_BUFLEN1`.
pub const DC_TXCTL_BUFLEN1: u32 = 0x000007FF;
/// `DC_TXCTL_BUFLEN2`.
pub const DC_TXCTL_BUFLEN2: u32 = 0x003FF800;
/// `DC_TXCTL_FILTTYPE0`.
pub const DC_TXCTL_FILTTYPE0: u32 = 0x00400000;
/// `DC_TXCTL_PAD`.
pub const DC_TXCTL_PAD: u32 = 0x00800000;
/// `DC_TXCTL_TLINK`.
pub const DC_TXCTL_TLINK: u32 = 0x01000000;
/// `DC_TXCTL_TLAST`.
pub const DC_TXCTL_TLAST: u32 = 0x02000000;
/// `DC_TXCTL_NOCRC`.
pub const DC_TXCTL_NOCRC: u32 = 0x04000000;
/// `DC_TXCTL_SETUP`.
pub const DC_TXCTL_SETUP: u32 = 0x08000000;
/// `DC_TXCTL_FILTTYPE1`.
pub const DC_TXCTL_FILTTYPE1: u32 = 0x10000000;
/// `DC_TXCTL_FIRSTFRAG`.
pub const DC_TXCTL_FIRSTFRAG: u32 = 0x20000000;
/// `DC_TXCTL_LASTFRAG`.
pub const DC_TXCTL_LASTFRAG: u32 = 0x40000000;
/// `DC_TXCTL_FINT`.
pub const DC_TXCTL_FINT: u32 = 0x80000000;
/// `DC_FILTER_PERFECT`.
pub const DC_FILTER_PERFECT: u32 = 0x00000000;
/// `DC_FILTER_HASHPERF`.
pub const DC_FILTER_HASHPERF: u32 = 0x00400000;
/// `DC_FILTER_INVERSE`.
pub const DC_FILTER_INVERSE: u32 = 0x10000000;
/// `DC_FILTER_HASHONLY`.
pub const DC_FILTER_HASHONLY: u32 = 0x10400000;
/// `DC_MAXFRAGS`.
pub const DC_MAXFRAGS: usize = 16;
/// `DC_RX_LIST_CNT`.
pub const DC_RX_LIST_CNT: usize = 64;
/// `DC_TX_LIST_CNT`.
pub const DC_TX_LIST_CNT: usize = 256;
/// `DC_MII_STARTDELIM`.
pub const DC_MII_STARTDELIM: u8 = 0x01;
/// `DC_MII_READOP`.
pub const DC_MII_READOP: u8 = 0x02;
/// `DC_MII_WRITEOP`.
pub const DC_MII_WRITEOP: u8 = 0x01;
/// `DC_MII_TURNAROUND`.
pub const DC_MII_TURNAROUND: u8 = 0x02;
/// `DC_AL_CR`: Command register.
pub const DC_AL_CR: u32 = 0x88;
/// `DC_AL_PAR0`: station address.
pub const DC_AL_PAR0: u32 = 0xA4;
/// `DC_AL_PAR1`: station address.
pub const DC_AL_PAR1: u32 = 0xA8;
/// `DC_AL_MAR0`: multicast hash filter.
pub const DC_AL_MAR0: u32 = 0xAC;
/// `DC_AL_MAR1`: multicast hash filter.
pub const DC_AL_MAR1: u32 = 0xB0;
/// `DC_AL_BMCR`: built in PHY control.
pub const DC_AL_BMCR: u32 = 0xB4;
/// `DC_AL_BMSR`: built in PHY status.
pub const DC_AL_BMSR: u32 = 0xB8;
/// `DC_AL_VENID`: built in PHY ID0.
pub const DC_AL_VENID: u32 = 0xBC;
/// `DC_AL_DEVID`: built in PHY ID1.
pub const DC_AL_DEVID: u32 = 0xC0;
/// `DC_AL_ANAR`: built in PHY autoneg advert.
pub const DC_AL_ANAR: u32 = 0xC4;
/// `DC_AL_LPAR`: bnilt in PHY link part. ability.
pub const DC_AL_LPAR: u32 = 0xC8;
/// `DC_AL_ANER`: built in PHY autoneg expansion.
pub const DC_AL_ANER: u32 = 0xCC;
/// `DC_ADMTEK_PHYADDR`.
pub const DC_ADMTEK_PHYADDR: i32 = 0x1;
/// `DC_AL_EE_NODEADDR`.
pub const DC_AL_EE_NODEADDR: i32 = 8;
/// `DC_AL_CR_ATUR`: Enable automatic TX underrun recovery.
pub const DC_AL_CR_ATUR: u32 = 0x00000001;
/// `DC_AX_FILTIDX`: RX filter index.
pub const DC_AX_FILTIDX: u32 = 0x68;
/// `DC_AX_FILTDATA`: RX filter data.
pub const DC_AX_FILTDATA: u32 = 0x70;
/// `DC_AX_NETCFG_RX_BROAD`.
pub const DC_AX_NETCFG_RX_BROAD: u32 = 0x00000100;
/// `DC_AX_FILTIDX_PAR0`.
pub const DC_AX_FILTIDX_PAR0: u32 = 0x00000000;
/// `DC_AX_FILTIDX_PAR1`.
pub const DC_AX_FILTIDX_PAR1: u32 = 0x00000001;
/// `DC_AX_FILTIDX_MAR0`.
pub const DC_AX_FILTIDX_MAR0: u32 = 0x00000002;
/// `DC_AX_FILTIDX_MAR1`.
pub const DC_AX_FILTIDX_MAR1: u32 = 0x00000003;
/// `DC_MX_MAGICPACKET`.
pub const DC_MX_MAGICPACKET: u32 = 0x80;
/// `DC_MX_NWAYSTAT`.
pub const DC_MX_NWAYSTAT: u32 = 0xA0;
/// `DC_MX_MPACK_DISABLE`.
pub const DC_MX_MPACK_DISABLE: u32 = 0x00400000;
/// `DC_MX_NWAY_10BTHALF`.
pub const DC_MX_NWAY_10BTHALF: u32 = 0x08000000;
/// `DC_MX_NWAY_10BTFULL`.
pub const DC_MX_NWAY_10BTFULL: u32 = 0x10000000;
/// `DC_MX_NWAY_100BTHALF`.
pub const DC_MX_NWAY_100BTHALF: u32 = 0x20000000;
/// `DC_MX_NWAY_100BTFULL`.
pub const DC_MX_NWAY_100BTFULL: u32 = 0x40000000;
/// `DC_MX_NWAY_100BT4`.
pub const DC_MX_NWAY_100BT4: u32 = 0x80000000;
/// `DC_MX_MAGIC_98713`.
pub const DC_MX_MAGIC_98713: u32 = 0x0F370000;
/// `DC_MX_MAGIC_98713A`.
pub const DC_MX_MAGIC_98713A: u32 = 0x0B3C0000;
/// `DC_MX_MAGIC_98715`.
pub const DC_MX_MAGIC_98715: u32 = 0x0B3C0000;
/// `DC_MX_MAGIC_98725`.
pub const DC_MX_MAGIC_98725: u32 = 0x0B3C0000;
/// `DC_PN_GPIO`: general purpose pins control.
pub const DC_PN_GPIO: u32 = 0x60;
/// `DC_PN_PWRUP_CFG`: config register, set by EEPROM.
pub const DC_PN_PWRUP_CFG: u32 = 0x90;
/// `DC_PN_SIOCTL`: serial EEPROM control register.
pub const DC_PN_SIOCTL: u32 = 0x98;
/// `DC_PN_MII`: MII access register.
pub const DC_PN_MII: u32 = 0xA0;
/// `DC_PN_NWAY`: Internal NWAY register.
pub const DC_PN_NWAY: u32 = 0xB8;
/// `DC_PN_SIOCTL_DATA`.
pub const DC_PN_SIOCTL_DATA: u32 = 0x0000003F;
/// `DC_PN_SIOCTL_OPCODE`.
pub const DC_PN_SIOCTL_OPCODE: u32 = 0x00000300;
/// `DC_PN_SIOCTL_BUSY`.
pub const DC_PN_SIOCTL_BUSY: u32 = 0x80000000;
/// `DC_PN_EEOPCODE_ERASE`.
pub const DC_PN_EEOPCODE_ERASE: u32 = 0x00000300;
/// `DC_PN_EEOPCODE_READ`.
pub const DC_PN_EEOPCODE_READ: u32 = 0x00000600;
/// `DC_PN_EEOPCODE_WRITE`.
pub const DC_PN_EEOPCODE_WRITE: u32 = 0x00000100;
/// `DC_PN_GPIO_DATA0`.
pub const DC_PN_GPIO_DATA0: u32 = 0x00000001;
/// `DC_PN_GPIO_DATA1`.
pub const DC_PN_GPIO_DATA1: u32 = 0x00000002;
/// `DC_PN_GPIO_DATA2`.
pub const DC_PN_GPIO_DATA2: u32 = 0x00000004;
/// `DC_PN_GPIO_DATA3`.
pub const DC_PN_GPIO_DATA3: u32 = 0x00000008;
/// `DC_PN_GPIO_CTL0`.
pub const DC_PN_GPIO_CTL0: u32 = 0x00000010;
/// `DC_PN_GPIO_CTL1`.
pub const DC_PN_GPIO_CTL1: u32 = 0x00000020;
/// `DC_PN_GPIO_CTL2`.
pub const DC_PN_GPIO_CTL2: u32 = 0x00000040;
/// `DC_PN_GPIO_CTL3`.
pub const DC_PN_GPIO_CTL3: u32 = 0x00000080;
/// `DC_PN_MII_DATA`.
pub const DC_PN_MII_DATA: u32 = 0x0000FFFF;
/// `DC_PN_MII_RESERVER`.
pub const DC_PN_MII_RESERVER: u32 = 0x00020000;
/// `DC_PN_MII_REGADDR`.
pub const DC_PN_MII_REGADDR: u32 = 0x007C0000;
/// `DC_PN_MII_PHYADDR`.
pub const DC_PN_MII_PHYADDR: u32 = 0x0F800000;
/// `DC_PN_MII_OPCODE`.
pub const DC_PN_MII_OPCODE: u32 = 0x30000000;
/// `DC_PN_MII_BUSY`.
pub const DC_PN_MII_BUSY: u32 = 0x80000000;
/// `DC_PN_MIIOPCODE_READ`.
pub const DC_PN_MIIOPCODE_READ: u32 = 0x60020000;
/// `DC_PN_MIIOPCODE_WRITE`.
pub const DC_PN_MIIOPCODE_WRITE: u32 = 0x50020000;
/// `DC_PN_NWAY_RESET`: reset.
pub const DC_PN_NWAY_RESET: u32 = 0x00000001;
/// `DC_PN_NWAY_PDOWN`: power down.
pub const DC_PN_NWAY_PDOWN: u32 = 0x00000002;
/// `DC_PN_NWAY_BYPASS`: bypass.
pub const DC_PN_NWAY_BYPASS: u32 = 0x00000004;
/// `DC_PN_NWAY_AUILOWCUR`: AUI low current.
pub const DC_PN_NWAY_AUILOWCUR: u32 = 0x00000008;
/// `DC_PN_NWAY_TPEXTEND`: low squelch voltage.
pub const DC_PN_NWAY_TPEXTEND: u32 = 0x00000010;
/// `DC_PN_NWAY_POLARITY`: 0 == on, 1 == off.
pub const DC_PN_NWAY_POLARITY: u32 = 0x00000020;
/// `DC_PN_NWAY_TP`: 1 == tp, 0 == AUI.
pub const DC_PN_NWAY_TP: u32 = 0x00000040;
/// `DC_PN_NWAY_AUIVOLT`: 1 == full, 0 == half.
pub const DC_PN_NWAY_AUIVOLT: u32 = 0x00000080;
/// `DC_PN_NWAY_DUPLEX`: LED, 1 == full, 0 == half.
pub const DC_PN_NWAY_DUPLEX: u32 = 0x00000100;
/// `DC_PN_NWAY_LINKTEST`: 0 == on, 1 == off.
pub const DC_PN_NWAY_LINKTEST: u32 = 0x00000200;
/// `DC_PN_NWAY_AUTODETECT`: 1 == off, 0 == on.
pub const DC_PN_NWAY_AUTODETECT: u32 = 0x00000400;
/// `DC_PN_NWAY_SPEEDSEL`: LED, 0 = 10, 1 == 100.
pub const DC_PN_NWAY_SPEEDSEL: u32 = 0x00000800;
/// `DC_PN_NWAY_NWAY_ENB`: 0 == off, 1 == on.
pub const DC_PN_NWAY_NWAY_ENB: u32 = 0x00001000;
/// `DC_PN_NWAY_CAP10HDX`.
pub const DC_PN_NWAY_CAP10HDX: u32 = 0x00002000;
/// `DC_PN_NWAY_CAP10FDX`.
pub const DC_PN_NWAY_CAP10FDX: u32 = 0x00004000;
/// `DC_PN_NWAY_CAP100FDX`.
pub const DC_PN_NWAY_CAP100FDX: u32 = 0x00008000;
/// `DC_PN_NWAY_CAP100HDX`.
pub const DC_PN_NWAY_CAP100HDX: u32 = 0x00010000;
/// `DC_PN_NWAY_CAP100T4`.
pub const DC_PN_NWAY_CAP100T4: u32 = 0x00020000;
/// `DC_PN_NWAY_ANEGRESTART`: resets when aneg done.
pub const DC_PN_NWAY_ANEGRESTART: u32 = 0x02000000;
/// `DC_PN_NWAY_REMFAULT`.
pub const DC_PN_NWAY_REMFAULT: u32 = 0x04000000;
/// `DC_PN_NWAY_LPAR10HDX`.
pub const DC_PN_NWAY_LPAR10HDX: u32 = 0x08000000;
/// `DC_PN_NWAY_LPAR10FDX`.
pub const DC_PN_NWAY_LPAR10FDX: u32 = 0x10000000;
/// `DC_PN_NWAY_LPAR100FDX`.
pub const DC_PN_NWAY_LPAR100FDX: u32 = 0x20000000;
/// `DC_PN_NWAY_LPAR100HDX`.
pub const DC_PN_NWAY_LPAR100HDX: u32 = 0x40000000;
/// `DC_PN_NWAY_LPAR100T4`.
pub const DC_PN_NWAY_LPAR100T4: u32 = 0x80000000;
/// `DC_CONEXANT_PHYADDR`.
pub const DC_CONEXANT_PHYADDR: i32 = 0x1;
/// `DC_CONEXANT_EE_NODEADDR`.
pub const DC_CONEXANT_EE_NODEADDR: usize = 0x19A;
/// `DC_TX_POLL`.
pub const DC_TX_POLL: u32 = 0x00000001;
/// `DC_TX_COALESCE`.
pub const DC_TX_COALESCE: u32 = 0x00000002;
/// `DC_TX_ADMTEK_WAR`.
pub const DC_TX_ADMTEK_WAR: u32 = 0x00000004;
/// `DC_TX_USE_TX_INTR`.
pub const DC_TX_USE_TX_INTR: u32 = 0x00000008;
/// `DC_RX_FILTER_TULIP`.
pub const DC_RX_FILTER_TULIP: u32 = 0x00000010;
/// `DC_TX_INTR_FIRSTFRAG`.
pub const DC_TX_INTR_FIRSTFRAG: u32 = 0x00000020;
/// `DC_PNIC_RX_BUG_WAR`.
pub const DC_PNIC_RX_BUG_WAR: u32 = 0x00000040;
/// `DC_TX_FIXED_RING`.
pub const DC_TX_FIXED_RING: u32 = 0x00000080;
/// `DC_TX_STORENFWD`.
pub const DC_TX_STORENFWD: u32 = 0x00000100;
/// `DC_REDUCED_MII_POLL`.
pub const DC_REDUCED_MII_POLL: u32 = 0x00000200;
/// `DC_TX_INTR_ALWAYS`.
pub const DC_TX_INTR_ALWAYS: u32 = 0x00000400;
/// `DC_21143_NWAY`.
pub const DC_21143_NWAY: u32 = 0x00000800;
/// `DC_128BIT_HASH`.
pub const DC_128BIT_HASH: u32 = 0x00001000;
/// `DC_64BIT_HASH`.
pub const DC_64BIT_HASH: u32 = 0x00002000;
/// `DC_TULIP_LEDS`.
pub const DC_TULIP_LEDS: u32 = 0x00004000;
/// `DC_TX_ONE`.
pub const DC_TX_ONE: u32 = 0x00008000;
/// `DC_TX_ALIGN`: align mbuf on tx.
pub const DC_TX_ALIGN: u32 = 0x00010000;
/// `DC_MOMENCO_BOTCH`.
pub const DC_MOMENCO_BOTCH: u32 = 0x00020000;
/// `DC_TIMEOUT`.
pub const DC_TIMEOUT: i32 = 1000;
/// `DC_REVISION_98713`.
pub const DC_REVISION_98713: u32 = 0x00;
/// `DC_REVISION_98713A`.
pub const DC_REVISION_98713A: u32 = 0x10;
/// `DC_REVISION_98715`.
pub const DC_REVISION_98715: u32 = 0x20;
/// `DC_REVISION_98715AEC_C`.
pub const DC_REVISION_98715AEC_C: u32 = 0x25;
/// `DC_REVISION_98725`.
pub const DC_REVISION_98725: u32 = 0x30;
/// `DC_REVISION_82C168`.
pub const DC_REVISION_82C168: u32 = 0x10;
/// `DC_REVISION_82C169`.
pub const DC_REVISION_82C169: u32 = 0x20;
/// `DC_REVISION_88140`.
pub const DC_REVISION_88140: u32 = 0x00;
/// `DC_REVISION_88141`.
pub const DC_REVISION_88141: u32 = 0x10;
/// `DC_REVISION_DM9102`.
pub const DC_REVISION_DM9102: u32 = 0x10;
/// `DC_REVISION_DM9102A`.
pub const DC_REVISION_DM9102A: u32 = 0x30;
/// `DC_PCI_CFID`: Id.
pub const DC_PCI_CFID: u32 = 0x00;
/// `DC_PCI_CFCS`: Command and status.
pub const DC_PCI_CFCS: u32 = 0x04;
/// `DC_PCI_CFRV`: Revision.
pub const DC_PCI_CFRV: u32 = 0x08;
/// `DC_PCI_CFLT`: Latency timer.
pub const DC_PCI_CFLT: u32 = 0x0C;
/// `DC_PCI_CFBIO`: Base I/O address.
pub const DC_PCI_CFBIO: u32 = 0x10;
/// `DC_PCI_CFBMA`: Base memory address.
pub const DC_PCI_CFBMA: u32 = 0x14;
/// `DC_PCI_CCIS`: Card info struct.
pub const DC_PCI_CCIS: u32 = 0x28;
/// `DC_PCI_CSID`: Subsystem ID.
pub const DC_PCI_CSID: u32 = 0x2C;
/// `DC_PCI_CBER`: Expansion ROM base address.
pub const DC_PCI_CBER: u32 = 0x30;
/// `DC_PCI_CCAP`: Caps pointer - PD/TD chip only.
pub const DC_PCI_CCAP: u32 = 0x34;
/// `DC_PCI_CFIT`: Interrupt.
pub const DC_PCI_CFIT: u32 = 0x3C;
/// `DC_PCI_CFDD`: Device and driver area.
pub const DC_PCI_CFDD: u32 = 0x40;
/// `DC_PCI_CWUA0`: Wake-Up LAN addr 0.
pub const DC_PCI_CWUA0: u32 = 0x44;
/// `DC_PCI_CWUA1`: Wake-Up LAN addr 1.
pub const DC_PCI_CWUA1: u32 = 0x48;
/// `DC_PCI_SOP0`: SecureON passwd 0.
pub const DC_PCI_SOP0: u32 = 0x4C;
/// `DC_PCI_SOP1`: SecureON passwd 1.
pub const DC_PCI_SOP1: u32 = 0x50;
/// `DC_PCI_CWUC`: Configuration Wake-Up cmd.
pub const DC_PCI_CWUC: u32 = 0x54;
/// `DC_PCI_CCID`: Capability ID - PD/TD only.
pub const DC_PCI_CCID: u32 = 0xDC;
/// `DC_PCI_CPMC`: Pwrmgmt ctl & sts - PD/TD only.
pub const DC_PCI_CPMC: u32 = 0xE0;
/// `DC_CFID_VENDOR`.
pub const DC_CFID_VENDOR: u32 = 0x0000FFFF;
/// `DC_CFID_DEVICE`.
pub const DC_CFID_DEVICE: u32 = 0xFFFF0000;
/// `DC_CFCS_IOSPACE`: I/O space enable.
pub const DC_CFCS_IOSPACE: u32 = 0x00000001;
/// `DC_CFCS_MEMSPACE`: memory space enable.
pub const DC_CFCS_MEMSPACE: u32 = 0x00000002;
/// `DC_CFCS_BUSMASTER`: bus master enable.
pub const DC_CFCS_BUSMASTER: u32 = 0x00000004;
/// `DC_CFCS_MWI_ENB`: mem write and inval enable.
pub const DC_CFCS_MWI_ENB: u32 = 0x00000010;
/// `DC_CFCS_PARITYERR_ENB`: parity error enable.
pub const DC_CFCS_PARITYERR_ENB: u32 = 0x00000040;
/// `DC_CFCS_SYSERR_ENB`: system error enable.
pub const DC_CFCS_SYSERR_ENB: u32 = 0x00000100;
/// `DC_CFCS_NEWCAPS`: new capabilities.
pub const DC_CFCS_NEWCAPS: u32 = 0x00100000;
/// `DC_CFCS_FAST_B2B`: fast back-to-back capable.
pub const DC_CFCS_FAST_B2B: u32 = 0x00800000;
/// `DC_CFCS_DATAPARITY`: Parity error report.
pub const DC_CFCS_DATAPARITY: u32 = 0x01000000;
/// `DC_CFCS_DEVSELTIM`: devsel timing.
pub const DC_CFCS_DEVSELTIM: u32 = 0x06000000;
/// `DC_CFCS_TGTABRT`: received target abort.
pub const DC_CFCS_TGTABRT: u32 = 0x10000000;
/// `DC_CFCS_MASTERABRT`: received master abort.
pub const DC_CFCS_MASTERABRT: u32 = 0x20000000;
/// `DC_CFCS_SYSERR`: asserted system error.
pub const DC_CFCS_SYSERR: u32 = 0x40000000;
/// `DC_CFCS_PARITYERR`: asserted parity error.
pub const DC_CFCS_PARITYERR: u32 = 0x80000000;
/// `DC_CFRV_STEPPING`.
pub const DC_CFRV_STEPPING: u32 = 0x0000000F;
/// `DC_CFRV_REVISION`.
pub const DC_CFRV_REVISION: u32 = 0x000000F0;
/// `DC_CFRV_SUBCLASS`.
pub const DC_CFRV_SUBCLASS: u32 = 0x00FF0000;
/// `DC_CFRV_BASECLASS`.
pub const DC_CFRV_BASECLASS: u32 = 0xFF000000;
/// `DC_21143_PB_REV`.
pub const DC_21143_PB_REV: u32 = 0x00000030;
/// `DC_21143_TB_REV`.
pub const DC_21143_TB_REV: u32 = 0x00000030;
/// `DC_21143_PC_REV`.
pub const DC_21143_PC_REV: u32 = 0x00000030;
/// `DC_21143_TC_REV`.
pub const DC_21143_TC_REV: u32 = 0x00000030;
/// `DC_21143_PD_REV`.
pub const DC_21143_PD_REV: u32 = 0x00000041;
/// `DC_21143_TD_REV`.
pub const DC_21143_TD_REV: u32 = 0x00000041;
/// `DC_CFLT_CACHELINESIZE`.
pub const DC_CFLT_CACHELINESIZE: u32 = 0x000000FF;
/// `DC_CFLT_LATENCYTIMER`.
pub const DC_CFLT_LATENCYTIMER: u32 = 0x0000FF00;
/// `DC_CSID_VENDOR`.
pub const DC_CSID_VENDOR: u32 = 0x0000FFFF;
/// `DC_CSID_DEVICE`.
pub const DC_CSID_DEVICE: u32 = 0xFFFF0000;
/// `DC_CCAP_OFFSET`.
pub const DC_CCAP_OFFSET: u32 = 0x000000FF;
/// `DC_CFIT_INTLINE`.
pub const DC_CFIT_INTLINE: u32 = 0x000000FF;
/// `DC_CFIT_INTPIN`.
pub const DC_CFIT_INTPIN: u32 = 0x0000FF00;
/// `DC_CFIT_MIN_GNT`.
pub const DC_CFIT_MIN_GNT: u32 = 0x00FF0000;
/// `DC_CFIT_MAX_LAT`.
pub const DC_CFIT_MAX_LAT: u32 = 0xFF000000;
/// `DC_CCID_CAPID`.
pub const DC_CCID_CAPID: u32 = 0x000000FF;
/// `DC_CCID_NEXTPTR`.
pub const DC_CCID_NEXTPTR: u32 = 0x0000FF00;
/// `DC_CCID_PM_VERS`.
pub const DC_CCID_PM_VERS: u32 = 0x00070000;
/// `DC_CCID_PME_CLK`.
pub const DC_CCID_PME_CLK: u32 = 0x00080000;
/// `DC_CCID_DVSPEC_INT`.
pub const DC_CCID_DVSPEC_INT: u32 = 0x00200000;
/// `DC_CCID_STATE_D1`.
pub const DC_CCID_STATE_D1: u32 = 0x02000000;
/// `DC_CCID_STATE_D2`.
pub const DC_CCID_STATE_D2: u32 = 0x04000000;
/// `DC_CCID_PME_D0`.
pub const DC_CCID_PME_D0: u32 = 0x08000000;
/// `DC_CCID_PME_D1`.
pub const DC_CCID_PME_D1: u32 = 0x10000000;
/// `DC_CCID_PME_D2`.
pub const DC_CCID_PME_D2: u32 = 0x20000000;
/// `DC_CCID_PME_D3HOT`.
pub const DC_CCID_PME_D3HOT: u32 = 0x40000000;
/// `DC_CCID_PME_D3COLD`.
pub const DC_CCID_PME_D3COLD: u32 = 0x80000000;
/// `DC_CPMC_STATE`.
pub const DC_CPMC_STATE: u32 = 0x00000003;
/// `DC_CPMC_PME_ENB`.
pub const DC_CPMC_PME_ENB: u32 = 0x00000100;
/// `DC_CPMC_PME_STS`.
pub const DC_CPMC_PME_STS: u32 = 0x00008000;
/// `DC_PSTATE_D0`.
pub const DC_PSTATE_D0: u32 = 0x0;
/// `DC_PSTATE_D1`.
pub const DC_PSTATE_D1: u32 = 0x1;
/// `DC_PSTATE_D2`.
pub const DC_PSTATE_D2: u32 = 0x2;
/// `DC_PSTATE_D3`.
pub const DC_PSTATE_D3: u32 = 0x3;
/// `DC_CFDD_DRVUSE`.
pub const DC_CFDD_DRVUSE: u32 = 0x0000FFFF;
/// `DC_CFDD_SNOOZE_MODE`.
pub const DC_CFDD_SNOOZE_MODE: u32 = 0x40000000;
/// `DC_CFDD_SLEEP_MODE`.
pub const DC_CFDD_SLEEP_MODE: u32 = 0x80000000;
/// `DC_CWUC_MUST_BE_ZERO`.
pub const DC_CWUC_MUST_BE_ZERO: u32 = 0x00000001;
/// `DC_CWUC_SECUREON_ENB`.
pub const DC_CWUC_SECUREON_ENB: u32 = 0x00000002;
/// `DC_CWUC_FORCE_WUL`.
pub const DC_CWUC_FORCE_WUL: u32 = 0x00000004;
/// `DC_CWUC_BNC_ABILITY`.
pub const DC_CWUC_BNC_ABILITY: u32 = 0x00000008;
/// `DC_CWUC_AUI_ABILITY`.
pub const DC_CWUC_AUI_ABILITY: u32 = 0x00000010;
/// `DC_CWUC_TP10_ABILITY`.
pub const DC_CWUC_TP10_ABILITY: u32 = 0x00000020;
/// `DC_CWUC_MII_ABILITY`.
pub const DC_CWUC_MII_ABILITY: u32 = 0x00000040;
/// `DC_CWUC_SYM_ABILITY`.
pub const DC_CWUC_SYM_ABILITY: u32 = 0x00000080;
/// `DC_CWUC_LOCK`.
pub const DC_CWUC_LOCK: u32 = 0x00000100;
/// `DC_IB_CTLRCNT`.
pub const DC_IB_CTLRCNT: usize = 0x13;
/// `DC_IB_LEAF0_CNUM`.
pub const DC_IB_LEAF0_CNUM: usize = 0x1A;
/// `DC_IB_LEAF0_OFFSET`.
pub const DC_IB_LEAF0_OFFSET: usize = 0x1B;
/// `DC_CTYPE_10BT`.
pub const DC_CTYPE_10BT: u16 = 0x0000;
/// `DC_CTYPE_10BT_NWAY`.
pub const DC_CTYPE_10BT_NWAY: u16 = 0x0100;
/// `DC_CTYPE_10BT_FDX`.
pub const DC_CTYPE_10BT_FDX: u16 = 0x0204;
/// `DC_CTYPE_10B2`.
pub const DC_CTYPE_10B2: u16 = 0x0001;
/// `DC_CTYPE_10B5`.
pub const DC_CTYPE_10B5: u16 = 0x0002;
/// `DC_CTYPE_100BT`.
pub const DC_CTYPE_100BT: u16 = 0x0003;
/// `DC_CTYPE_100BT_FDX`.
pub const DC_CTYPE_100BT_FDX: u16 = 0x0205;
/// `DC_CTYPE_100T4`.
pub const DC_CTYPE_100T4: u16 = 0x0006;
/// `DC_CTYPE_100FX`.
pub const DC_CTYPE_100FX: u16 = 0x0007;
/// `DC_CTYPE_100FX_FDX`.
pub const DC_CTYPE_100FX_FDX: u16 = 0x0208;
/// `DC_CTYPE_MII_10BT`.
pub const DC_CTYPE_MII_10BT: u16 = 0x0009;
/// `DC_CTYPE_MII_10BT_FDX`.
pub const DC_CTYPE_MII_10BT_FDX: u16 = 0x020A;
/// `DC_CTYPE_MII_100BT`.
pub const DC_CTYPE_MII_100BT: u16 = 0x000D;
/// `DC_CTYPE_MII_100BT_FDX`.
pub const DC_CTYPE_MII_100BT_FDX: u16 = 0x020E;
/// `DC_CTYPE_MII_100T4`.
pub const DC_CTYPE_MII_100T4: u16 = 0x000F;
/// `DC_CTYPE_MII_100FX`.
pub const DC_CTYPE_MII_100FX: u16 = 0x0010;
/// `DC_CTYPE_MII_100FX_FDX`.
pub const DC_CTYPE_MII_100FX_FDX: u16 = 0x0211;
/// `DC_CTYPE_DYN_PUP_AUTOSENSE`.
pub const DC_CTYPE_DYN_PUP_AUTOSENSE: u16 = 0x0800;
/// `DC_CTYPE_PUP_AUTOSENSE`.
pub const DC_CTYPE_PUP_AUTOSENSE: u16 = 0x8800;
/// `DC_CTYPE_NOMEDIA`.
pub const DC_CTYPE_NOMEDIA: u16 = 0xFFFF;
/// `DC_EBLOCK_SIA`.
pub const DC_EBLOCK_SIA: u8 = 0x0002;
/// `DC_EBLOCK_MII`.
pub const DC_EBLOCK_MII: u8 = 0x0003;
/// `DC_EBLOCK_SYM`.
pub const DC_EBLOCK_SYM: u8 = 0x0004;
/// `DC_EBLOCK_RESET`.
pub const DC_EBLOCK_RESET: u8 = 0x0005;
/// `DC_EBLOCK_PHY_SHUTDOWN`.
pub const DC_EBLOCK_PHY_SHUTDOWN: u8 = 0x0006;
/// `DC_SIA_CODE_10BT`.
pub const DC_SIA_CODE_10BT: u8 = 0x00;
/// `DC_SIA_CODE_10B2`.
pub const DC_SIA_CODE_10B2: u8 = 0x01;
/// `DC_SIA_CODE_10B5`.
pub const DC_SIA_CODE_10B5: u8 = 0x02;
/// `DC_SIA_CODE_10BT_FDX`.
pub const DC_SIA_CODE_10BT_FDX: u8 = 0x04;
/// `DC_SIA_CODE_EXT`.
pub const DC_SIA_CODE_EXT: u8 = 0x40;
/// `DC_SYM_CODE_100BT`.
pub const DC_SYM_CODE_100BT: u8 = 0x03;
/// `DC_SYM_CODE_100BT_FDX`.
pub const DC_SYM_CODE_100BT_FDX: u8 = 0x05;
/// `DC_SYM_CODE_100T4`.
pub const DC_SYM_CODE_100T4: u8 = 0x06;
/// `DC_SYM_CODE_100FX`.
pub const DC_SYM_CODE_100FX: u8 = 0x07;
/// `DC_SYM_CODE_100FX_FDX`.
pub const DC_SYM_CODE_100FX_FDX: u8 = 0x08;

/// `DC_INTRS`: the interrupts dc(4) enables.
pub const DC_INTRS: u32 = DC_IMR_RX_OK
    | DC_IMR_TX_OK
    | DC_IMR_RX_NOBUF
    | DC_IMR_RX_WATDOGTIMEO
    | DC_IMR_TX_IDLE
    | DC_IMR_TX_NOBUF
    | DC_IMR_TX_UNDERRUN
    | DC_IMR_BUS_ERR
    | DC_IMR_ABNORMAL
    | DC_IMR_NORMAL/*|DC_IMR_TX_EARLY*/;

/// `DC_RXSTAT`.
pub const DC_RXSTAT: u32 = DC_RXSTAT_FIRSTFRAG | DC_RXSTAT_LASTFRAG | DC_RXSTAT_OWN;

/// `DC_PN_GPIO_SPEEDSEL`: 1 == 100Mbps, 0 == 10Mbps.
pub const DC_PN_GPIO_SPEEDSEL: u32 = DC_PN_GPIO_DATA0;
/// `DC_PN_GPIO_100TX_LOOP`: 1 == normal, 0 == loop.
pub const DC_PN_GPIO_100TX_LOOP: u32 = DC_PN_GPIO_DATA1;
/// `DC_PN_GPIO_BNC_ENB`.
pub const DC_PN_GPIO_BNC_ENB: u32 = DC_PN_GPIO_DATA2;
/// `DC_PN_GPIO_100TX_LNK`.
pub const DC_PN_GPIO_100TX_LNK: u32 = DC_PN_GPIO_DATA3;

/// `struct dc_desc`: a 21x4x transmit or receive descriptor (`dc_data` is `dc_ptr1`,
/// `dc_next` is `dc_ptr2`). Little-endian in memory.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DcDesc {
    /// `dc_status`.
    pub dc_status: u32,
    /// `dc_ctl`.
    pub dc_ctl: u32,
    /// `dc_ptr1` (`dc_data`).
    pub dc_ptr1: u32,
    /// `dc_ptr2` (`dc_next`).
    pub dc_ptr2: u32,
    /// `dc_pad`.
    pub dc_pad: [u32; 4],
}

/// A word of a [`DcDesc`], for the softc's descriptor accessors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DcDescWord {
    /// `dc_status`.
    Status,
    /// `dc_ctl`.
    Ctl,
    /// `dc_data` (`dc_ptr1`).
    Data,
    /// `dc_next` (`dc_ptr2`).
    Next,
}

impl DcDescWord {
    /// The word's byte offset in a [`DcDesc`].
    pub const fn offset(self) -> usize {
        match self {
            DcDescWord::Status => offset_of!(DcDesc, dc_status),
            DcDescWord::Ctl => offset_of!(DcDesc, dc_ctl),
            DcDescWord::Data => offset_of!(DcDesc, dc_ptr1),
            DcDescWord::Next => offset_of!(DcDesc, dc_ptr2),
        }
    }
}

/// `struct dc_list_data`: the receive and transmit rings, the setup frame and a pad, in one
/// DMA allocation.
#[repr(C)]
pub struct DcListData {
    /// `dc_rx_list`.
    pub dc_rx_list: [DcDesc; DC_RX_LIST_CNT],
    /// `dc_tx_list`.
    pub dc_tx_list: [DcDesc; DC_TX_LIST_CNT],
    /// `dc_sbuf`: the setup frame.
    pub dc_sbuf: [u32; DC_SFRAME_LEN / size_of::<u32>()],
    /// `dc_pad`.
    pub dc_pad: [u8; ETHER_MIN_LEN],
}

/// `offsetof(struct dc_list_data, dc_sbuf)`.
pub const DC_SBUF_OFF: usize = offset_of!(DcListData, dc_sbuf);

/// `struct dc_swdesc`: software descriptor.
#[derive(Default)]
pub struct DcSwdesc {
    /// `sd_map`.
    pub sd_map: Cell<Option<&'static BusDmamap>>,
    /// `sd_mbuf`.
    pub sd_mbuf: Cell<Option<&'static Mbuf>>,
    /// The C's `sd_mbuf = (struct mbuf *)&dc_sbuf[0]`: the slot holds the setup frame.
    pub sd_setup: Cell<bool>,
}

impl DcSwdesc {
    /// `sd->sd_map`, which `dc_attach` created.
    pub fn map(&self) -> &'static BusDmamap {
        match self.sd_map.get() {
            Some(m) => m,
            None => panic(format_args!("dc: descriptor slot without a dma map")),
        }
    }

    /// Whether `sd_mbuf` is set (an mbuf or the setup frame).
    pub fn has_mbuf(&self) -> bool {
        self.sd_mbuf.get().is_some() || self.sd_setup.get()
    }

    /// `sd_mbuf = NULL`.
    pub fn clear_mbuf(&self) {
        self.sd_mbuf.set(None);
        self.sd_setup.set(false);
    }
}

/// `struct dc_chain_data`.
pub struct DcChainData {
    /// `dc_rx_chain`.
    pub dc_rx_chain: [DcSwdesc; DC_RX_LIST_CNT],
    /// `dc_tx_chain`.
    pub dc_tx_chain: [DcSwdesc; DC_TX_LIST_CNT],
    /// `dc_tx_prod`.
    pub dc_tx_prod: Cell<usize>,
    /// `dc_tx_cons`.
    pub dc_tx_cons: Cell<usize>,
    /// `dc_tx_cnt`.
    pub dc_tx_cnt: Cell<usize>,
    /// `dc_rx_prod`.
    pub dc_rx_prod: Cell<usize>,
}

impl DcChainData {
    /// `&cd->dc_rx_chain[i]`.
    pub fn rx(&self, i: usize) -> &DcSwdesc {
        match self.dc_rx_chain.get(i) {
            Some(s) => s,
            None => panic(format_args!("dc: bad rx slot {i}")),
        }
    }

    /// `&cd->dc_tx_chain[i]`.
    pub fn tx(&self, i: usize) -> &DcSwdesc {
        match self.dc_tx_chain.get(i) {
            Some(s) => s,
            None => panic(format_args!("dc: bad tx slot {i}")),
        }
    }
}

/// `struct dc_mediainfo`: a media of the 21143's SROM and its GPIO and reset sequences.
#[derive(Clone, Copy, Debug, Default)]
pub struct DcMediainfo {
    /// `dc_media`.
    pub dc_media: u64,
    /// `dc_gp_ptr`, as an offset into `dc_srom`.
    pub dc_gp_ptr: usize,
    /// `dc_gp_len`: the number of 16-bit words at `dc_gp_ptr`.
    pub dc_gp_len: u8,
    /// `dc_reset_ptr`, as an offset into `dc_srom`.
    pub dc_reset_ptr: usize,
    /// `dc_reset_len`: the number of 16-bit words at `dc_reset_ptr`.
    pub dc_reset_len: u8,
    /// `dc_next`.
    pub dc_next: Option<NonNull<DcMediainfo>>,
}

/// `struct dc_type`: a vendor and product the PCI front-end matches.
#[derive(Clone, Copy, Debug)]
pub struct DcType {
    /// `dc_vid`.
    pub dc_vid: u16,
    /// `dc_did`.
    pub dc_did: u16,
}

/// `struct dc_mii_frame`: a bit-banged MII management frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct DcMiiFrame {
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

/// `struct dc_info_leaf`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DcInfoLeaf {
    /// `dc_conntype`.
    pub dc_conntype: u16,
    /// `dc_blkcnt`.
    pub dc_blkcnt: u8,
    /// `dc_rsvd`.
    pub dc_rsvd: u8,
    /// `dc_infoblk`.
    pub dc_infoblk: u16,
}

/// `struct dc_leaf_hdr`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DcLeafHdr {
    /// `dc_mtype`.
    pub dc_mtype: u16,
    /// `dc_mcnt`.
    pub dc_mcnt: u8,
    /// `dc_rsvd`.
    pub dc_rsvd: u8,
}

/// `struct dc_eblock_hdr`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DcEblockHdr {
    /// `dc_len`.
    pub dc_len: u8,
    /// `dc_type`.
    pub dc_type: u8,
}

/// `struct dc_sia_ext`: an SIA block with media specific data
/// (`dc_sia_code & DC_SIA_CODE_EXT`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DcSiaExt {
    /// `dc_sia_mediaspec`: CSR13, CSR14, CSR15.
    pub dc_sia_mediaspec: [u8; 6],
    /// `dc_sia_gpio_ctl`.
    pub dc_sia_gpio_ctl: [u8; 2],
    /// `dc_sia_gpio_dat`.
    pub dc_sia_gpio_dat: [u8; 2],
}

/// `struct dc_sia_noext`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DcSiaNoext {
    /// `dc_sia_gpio_ctl`.
    pub dc_sia_gpio_ctl: [u8; 2],
    /// `dc_sia_gpio_dat`.
    pub dc_sia_gpio_dat: [u8; 2],
}

/// `dc_un` of `struct dc_eblock_sia`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union DcSiaUn {
    /// `dc_sia_ext`.
    pub dc_sia_ext: DcSiaExt,
    /// `dc_sia_noext`.
    pub dc_sia_noext: DcSiaNoext,
}

/// `struct dc_eblock_sia`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DcEblockSia {
    /// `dc_sia_hdr`.
    pub dc_sia_hdr: DcEblockHdr,
    /// `dc_sia_code`.
    pub dc_sia_code: u8,
    /// `dc_un`.
    pub dc_un: DcSiaUn,
}

/// `struct dc_eblock_mii`. The first word in the gpr and reset sequences is always a control
/// word; after the header come `u_int16_t dc_gpr_dat[dc_gpr_len]`, `u_int8_t dc_reset_len`,
/// `u_int16_t dc_reset_dat[dc_reset_len]`, and other fields we don't care about since they
/// can be determined by looking at the PHY.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DcEblockMii {
    /// `dc_mii_hdr`.
    pub dc_mii_hdr: DcEblockHdr,
    /// `dc_mii_phynum`.
    pub dc_mii_phynum: u8,
    /// `dc_gpr_len`.
    pub dc_gpr_len: u8,
}

/// `struct dc_eblock_sym`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DcEblockSym {
    /// `dc_sym_hdr`.
    pub dc_sym_hdr: DcEblockHdr,
    /// `dc_sym_code`.
    pub dc_sym_code: u8,
    /// `dc_sym_gpio_ctl`.
    pub dc_sym_gpio_ctl: [u8; 2],
    /// `dc_sym_gpio_dat`.
    pub dc_sym_gpio_dat: [u8; 2],
    /// `dc_sym_cmd`.
    pub dc_sym_cmd: [u8; 2],
}

/// `struct dc_eblock_reset`: after the header, `u_int16_t dc_reset_dat[dc_reset_len]`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DcEblockReset {
    /// `dc_reset_hdr`.
    pub dc_reset_hdr: DcEblockHdr,
    /// `dc_reset_len`.
    pub dc_reset_len: u8,
}

/// `struct dc_softc`.
///
/// dc(4) is not MP-safe: its interrupt is established without `IPL_MPSAFE` and its start
/// routine is the interface's `if_start`, so everything here changes under the kernel lock,
/// at `splnet`, as in C.
#[repr(C)]
pub struct DcSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_ih`.
    pub sc_ih: Cell<*mut c_void>,
    /// `sc_arpcom`: interface info.
    pub sc_arpcom: Arpcom,
    /// `sc_mii`.
    pub sc_mii: MiiData,
    /// `dc_bhandle`: bus space handle.
    pub dc_bhandle: Cell<Option<BusSpaceHandle>>,
    /// `dc_btag`: bus space tag.
    pub dc_btag: Cell<Option<BusSpaceTag>>,
    /// `dc_type`.
    pub dc_type: Cell<u8>,
    /// `dc_pmode`.
    pub dc_pmode: Cell<u8>,
    /// `dc_link`.
    pub dc_link: Cell<u8>,
    /// `dc_cachesize`.
    pub dc_cachesize: Cell<u8>,
    /// `dc_romwidth`.
    pub dc_romwidth: Cell<i32>,
    /// `dc_pnic_rx_bug_save`.
    pub dc_pnic_rx_bug_save: Cell<usize>,
    /// `dc_pnic_rx_buf`: `ETHER_MAX_DIX_LEN * 5` bytes (PNIC only).
    pub dc_pnic_rx_buf: Cell<*mut u8>,
    /// `dc_if_media`.
    pub dc_if_media: Cell<u64>,
    /// `dc_flags`.
    pub dc_flags: Cell<u32>,
    /// `dc_txthresh`.
    pub dc_txthresh: Cell<u32>,
    /// `dc_srom`.
    pub dc_srom: Cell<*mut u8>,
    /// `dc_sromsize`.
    pub dc_sromsize: Cell<usize>,
    /// `dc_mi`.
    pub dc_mi: Cell<Option<NonNull<DcMediainfo>>>,
    /// `dc_ldata`.
    pub dc_ldata: Cell<*mut DcListData>,
    /// `dc_cdata`.
    pub dc_cdata: DcChainData,
    /// `dc_csid`.
    pub dc_csid: Cell<u32>,
    /// `dc_revision`.
    pub dc_revision: Cell<u32>,
    /// `dc_tick_tmo`.
    pub dc_tick_tmo: Timeout,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_listmap`.
    pub sc_listmap: Cell<Option<&'static BusDmamap>>,
    /// `sc_listseg`.
    pub sc_listseg: Cell<[BusDmaSegment; 1]>,
    /// `sc_listnseg`.
    pub sc_listnseg: Cell<usize>,
    /// `sc_listkva`.
    pub sc_listkva: Cell<*mut u8>,
    /// `sc_rx_sparemap`.
    pub sc_rx_sparemap: Cell<Option<&'static BusDmamap>>,
    /// `sc_tx_sparemap`.
    pub sc_tx_sparemap: Cell<Option<&'static BusDmamap>>,
    /// `sc_hasmac`.
    pub sc_hasmac: Cell<i32>,
}

impl DcSoftc {
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

    /// `sc->dc_btag` and `sc->dc_bhandle`, which the bus front-end maps first.
    pub fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.dc_btag.get(), self.dc_bhandle.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("{}: registers not mapped", self.devname())),
        }
    }

    /// `CSR_WRITE_4(sc, reg, val)`.
    pub fn csr_write_4(&self, reg: u32, val: u32) {
        let (t, h) = self.regs();
        bus_space_write_4(t, h, reg as BusSize, val);
    }

    /// `CSR_READ_4(sc, reg)`.
    pub fn csr_read_4(&self, reg: u32) -> u32 {
        let (t, h) = self.regs();
        bus_space_read_4(t, h, reg as BusSize)
    }

    /// `DC_SETBIT(sc, reg, x)` (dc.c, dcphy.c).
    pub fn dc_setbit(&self, reg: u32, x: u32) {
        self.csr_write_4(reg, self.csr_read_4(reg) | x);
    }

    /// `DC_CLRBIT(sc, reg, x)` (dc.c, dcphy.c).
    pub fn dc_clrbit(&self, reg: u32, x: u32) {
        self.csr_write_4(reg, self.csr_read_4(reg) & !x);
    }

    /// `DC_PN_GPIO_SETBIT(sc, r)`.
    pub fn dc_pn_gpio_setbit(&self, r: u32) {
        self.dc_setbit(DC_PN_GPIO, r | (r << 4));
    }

    /// `DC_PN_GPIO_CLRBIT(sc, r)`.
    pub fn dc_pn_gpio_clrbit(&self, r: u32) {
        self.dc_setbit(DC_PN_GPIO, r << 4);
        self.dc_clrbit(DC_PN_GPIO, r);
    }

    /// `sc->dc_flags & f`, as a truth value.
    pub fn has_flags(&self, f: u32) -> bool {
        self.dc_flags.get() & f != 0
    }

    /// `sc->dc_flags |= f`.
    pub fn set_flags(&self, f: u32) {
        self.dc_flags.set(self.dc_flags.get() | f);
    }

    /// `DC_IS_MACRONIX(sc)`.
    pub fn dc_is_macronix(&self) -> bool {
        matches!(
            self.dc_type.get(),
            DC_TYPE_98713 | DC_TYPE_98713A | DC_TYPE_987x5
        )
    }

    /// `DC_IS_ADMTEK(sc)`.
    pub fn dc_is_admtek(&self) -> bool {
        matches!(self.dc_type.get(), DC_TYPE_AL981 | DC_TYPE_AN983)
    }

    /// `DC_IS_CENTAUR(sc)`.
    pub fn dc_is_centaur(&self) -> bool {
        self.dc_type.get() == DC_TYPE_AN983
    }

    /// `DC_IS_INTEL(sc)`.
    pub fn dc_is_intel(&self) -> bool {
        matches!(self.dc_type.get(), DC_TYPE_21143 | DC_TYPE_21145)
    }

    /// `DC_IS_ASIX(sc)`.
    pub fn dc_is_asix(&self) -> bool {
        self.dc_type.get() == DC_TYPE_ASIX
    }

    /// `DC_IS_COMET(sc)`.
    pub fn dc_is_comet(&self) -> bool {
        self.dc_type.get() == DC_TYPE_AL981
    }

    /// `DC_IS_DAVICOM(sc)`.
    pub fn dc_is_davicom(&self) -> bool {
        self.dc_type.get() == DC_TYPE_DM9102
    }

    /// `DC_IS_PNICII(sc)`.
    pub fn dc_is_pnicii(&self) -> bool {
        self.dc_type.get() == DC_TYPE_PNICII
    }

    /// `DC_IS_PNIC(sc)`.
    pub fn dc_is_pnic(&self) -> bool {
        self.dc_type.get() == DC_TYPE_PNIC
    }

    /// `DC_IS_XIRCOM(sc)`.
    pub fn dc_is_xircom(&self) -> bool {
        self.dc_type.get() == DC_TYPE_XIRCOM
    }

    /// `DC_IS_CONEXANT(sc)`.
    pub fn dc_is_conexant(&self) -> bool {
        self.dc_type.get() == DC_TYPE_CONEXANT
    }

    /// `DC_HAS_BROKEN_RXSTATE(sc)`.
    pub fn dc_has_broken_rxstate(&self) -> bool {
        self.dc_is_centaur()
            || self.dc_is_conexant()
            || (self.dc_is_davicom() && self.dc_revision.get() >= DC_REVISION_DM9102A)
    }

    /// `sc->sc_listmap`, which `dc_attach` loaded.
    pub fn listmap(&self) -> &'static BusDmamap {
        match self.sc_listmap.get() {
            Some(m) => m,
            None => panic(format_args!("{}: no list map", self.devname())),
        }
    }

    /// `sc->sc_listmap->dm_segs[0].ds_addr`: the bus address of `struct dc_list_data`.
    pub fn list_addr(&self) -> u32 {
        self.listmap().dm_segs()[0].get().ds_addr as u32
    }

    /// `sc->sc_rx_sparemap`, which `dc_attach` created.
    pub fn rx_sparemap(&self) -> &'static BusDmamap {
        match self.sc_rx_sparemap.get() {
            Some(m) => m,
            None => panic(format_args!("{}: no rx spare map", self.devname())),
        }
    }

    /// `sc->sc_tx_sparemap`, which `dc_attach` created.
    pub fn tx_sparemap(&self) -> &'static BusDmamap {
        match self.sc_tx_sparemap.get() {
            Some(m) => m,
            None => panic(format_args!("{}: no tx spare map", self.devname())),
        }
    }

    /// The byte at `off` of the mapped `struct dc_list_data`, checked against its size.
    fn ldata_ptr(&self, off: usize, len: usize) -> *mut u8 {
        let base = self.dc_ldata.get();
        if base.is_null() || off + len > size_of::<DcListData>() {
            panic(format_args!(
                "{}: bad list data access at {off}",
                self.devname()
            ));
        }
        // SAFETY: dc_attach mapped a whole `struct dc_list_data` at `base`; `off + len` is
        // within it.
        unsafe { base.cast::<u8>().add(off) }
    }

    /// A descriptor word, read volatile and returned in host order.
    fn desc_read(&self, off: usize) -> u32 {
        let p = self.ldata_ptr(off, size_of::<u32>()).cast::<u32>();
        // SAFETY: a word of the mapped list data (`ldata_ptr`), 4-byte aligned (the list is
        // page aligned and every member a multiple of 4 bytes in).
        u32::from_le(unsafe { ptr::read_volatile(p) })
    }

    /// A descriptor word, written volatile from host order.
    fn desc_write(&self, off: usize, v: u32) {
        let p = self.ldata_ptr(off, size_of::<u32>()).cast::<u32>();
        // SAFETY: as in `desc_read`.
        unsafe { ptr::write_volatile(p, v.to_le()) }
    }

    /// `letoh32(sc->dc_ldata->dc_rx_list[i].<w>)`.
    pub fn rxd_read(&self, i: usize, w: DcDescWord) -> u32 {
        if i >= DC_RX_LIST_CNT {
            panic(format_args!("{}: bad rx descriptor {i}", self.devname()));
        }
        self.desc_read(dc_rx_list_off(i) + w.offset())
    }

    /// `sc->dc_ldata->dc_rx_list[i].<w> = htole32(v)`.
    pub fn rxd_write(&self, i: usize, w: DcDescWord, v: u32) {
        if i >= DC_RX_LIST_CNT {
            panic(format_args!("{}: bad rx descriptor {i}", self.devname()));
        }
        self.desc_write(dc_rx_list_off(i) + w.offset(), v);
    }

    /// `letoh32(sc->dc_ldata->dc_tx_list[i].<w>)`.
    pub fn txd_read(&self, i: usize, w: DcDescWord) -> u32 {
        if i >= DC_TX_LIST_CNT {
            panic(format_args!("{}: bad tx descriptor {i}", self.devname()));
        }
        self.desc_read(dc_tx_list_off(i) + w.offset())
    }

    /// `sc->dc_ldata->dc_tx_list[i].<w> = htole32(v)`.
    pub fn txd_write(&self, i: usize, w: DcDescWord, v: u32) {
        if i >= DC_TX_LIST_CNT {
            panic(format_args!("{}: bad tx descriptor {i}", self.devname()));
        }
        self.desc_write(dc_tx_list_off(i) + w.offset(), v);
    }

    /// `bzero(&sc->dc_ldata->dc_rx_list, sizeof(sc->dc_ldata->dc_rx_list))`.
    pub fn rx_list_zero(&self) {
        let len = DC_RX_LIST_CNT * size_of::<DcDesc>();
        let p = self.ldata_ptr(dc_rx_list_off(0), len);
        // SAFETY: the receive ring of the mapped list data (`ldata_ptr`).
        unsafe { ptr::write_bytes(p, 0, len) }
    }

    /// `bzero(&sc->dc_ldata->dc_tx_list, sizeof(sc->dc_ldata->dc_tx_list))`.
    pub fn tx_list_zero(&self) {
        let len = DC_TX_LIST_CNT * size_of::<DcDesc>();
        let p = self.ldata_ptr(dc_tx_list_off(0), len);
        // SAFETY: the transmit ring of the mapped list data (`ldata_ptr`).
        unsafe { ptr::write_bytes(p, 0, len) }
    }

    /// `bzero(sc->dc_ldata->dc_sbuf, DC_SFRAME_LEN)`.
    pub fn sbuf_zero(&self) {
        let p = self.ldata_ptr(DC_SBUF_OFF, DC_SFRAME_LEN);
        // SAFETY: the setup frame of the mapped list data (`ldata_ptr`).
        unsafe { ptr::write_bytes(p, 0, DC_SFRAME_LEN) }
    }

    /// `sc->dc_ldata->dc_sbuf[i]`, as stored (the setup frame is little-endian words).
    pub fn sbuf_get(&self, i: usize) -> u32 {
        let p = self
            .ldata_ptr(DC_SBUF_OFF + i * size_of::<u32>(), size_of::<u32>())
            .cast::<u32>();
        // SAFETY: a word of the setup frame (`ldata_ptr`), aligned as in `desc_read`.
        unsafe { ptr::read_volatile(p) }
    }

    /// `sc->dc_ldata->dc_sbuf[i] = v`, stored as given.
    pub fn sbuf_set(&self, i: usize, v: u32) {
        let p = self
            .ldata_ptr(DC_SBUF_OFF + i * size_of::<u32>(), size_of::<u32>())
            .cast::<u32>();
        // SAFETY: as in `sbuf_get`.
        unsafe { ptr::write_volatile(p, v) }
    }

    /// The SROM `dc_read_srom` read, as a slice (empty when there is none).
    pub fn srom(&self) -> &[u8] {
        let p = self.dc_srom.get();
        if p.is_null() {
            return &[];
        }
        // SAFETY: dc_read_srom allocated `dc_sromsize` bytes at `dc_srom` and filled them;
        // they stay until dc_detach frees them with the device.
        unsafe { core::slice::from_raw_parts(p, self.dc_sromsize.get()) }
    }
}

// SAFETY: `#[repr(C)]` with the device first; the arpcom, the MII data and the timeout are
// all-zero valid, and every other member is a `Cell` of an integer, a bool, a pointer, a
// segment (integers) or an `Option`, or an array of such.
unsafe impl Softc for DcSoftc {}

/// `DC_RXBYTES(x)`: the frame length of a receive status word.
pub const fn dc_rxbytes(x: u32) -> u32 {
    (x & DC_RXSTAT_RXLEN) >> 16
}

/// `DC_INC(x, y)`: the ring index after `x` in a ring of `y`.
pub const fn dc_inc(x: usize, y: usize) -> usize {
    (x + 1) % y
}

/// `DC_SP_FIELD(x, f)`: the `f`th 16-bit word of the station address `x` as a setup frame
/// word (`DC_SP_FIELD_C`: shifted to the upper half on big-endian machines).
pub fn dc_sp_field(x: &[u8; 6], f: usize) -> u32 {
    let w = u32::from(u16::from_ne_bytes([x[2 * f], x[2 * f + 1]]));
    if cfg!(target_endian = "big") {
        w << 16
    } else {
        w
    }
}

/// `offsetof(struct dc_list_data, dc_rx_list[i])`.
pub const fn dc_rx_list_off(i: usize) -> usize {
    offset_of!(DcListData, dc_rx_list) + i * size_of::<DcDesc>()
}

/// `offsetof(struct dc_list_data, dc_tx_list[i])`.
pub const fn dc_tx_list_off(i: usize) -> usize {
    offset_of!(DcListData, dc_tx_list) + i * size_of::<DcDesc>()
}

const _: () = {
    assert!(size_of::<DcDesc>() == 32);
    assert!(DC_SBUF_OFF == (DC_RX_LIST_CNT + DC_TX_LIST_CNT) * 32);
    assert!(size_of::<DcEblockSia>() == 13);
    assert!(size_of::<DcEblockMii>() == 4);
    assert!(size_of::<DcLeafHdr>() == 4);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    /// Every define of the header, against the C (`just test-ref`).
    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn defines_match_reference() {
        let defs = crate::reftest::defines("sys/dev/ic/dcreg.h");
        let mut ours = crate::reftest::assert_defines!(defs;
            DC_BUSCTL, DC_TXSTART, DC_RXSTART, DC_RXADDR, DC_TXADDR, DC_ISR, DC_NETCFG, DC_IMR, DC_FRAMESDISCARDED, DC_SIO, DC_ROM, DC_TIMER, DC_10BTSTAT, DC_SIARESET, DC_10BTCTRL, DC_WATCHDOG, DC_SIAGP, DC_TYPE_98713, DC_TYPE_98713A, DC_TYPE_987x5, DC_TYPE_21143, DC_TYPE_ASIX, DC_TYPE_AL981, DC_TYPE_AN983, DC_TYPE_DM9102, DC_TYPE_PNICII, DC_TYPE_PNIC, DC_TYPE_XIRCOM, DC_TYPE_CONEXANT, DC_TYPE_21145, DC_PMODE_MII, DC_PMODE_SYM, DC_PMODE_SIA, DC_BUSCTL_RESET, DC_BUSCTL_ARBITRATION, DC_BUSCTL_SKIPLEN, DC_BUSCTL_BUF_BIGENDIAN, DC_BUSCTL_BURSTLEN, DC_BUSCTL_CACHEALIGN, DC_BUSCTL_TXPOLL, DC_BUSCTL_DBO, DC_BUSCTL_MRME, DC_BUSCTL_MRLE, DC_BUSCTL_MWIE, DC_BUSCTL_ONNOW_ENB, DC_SKIPLEN_1LONG, DC_SKIPLEN_2LONG, DC_SKIPLEN_3LONG, DC_SKIPLEN_4LONG, DC_SKIPLEN_5LONG, DC_CACHEALIGN_NONE, DC_CACHEALIGN_8LONG, DC_CACHEALIGN_16LONG, DC_CACHEALIGN_32LONG, DC_BURSTLEN_USECA, DC_BURSTLEN_1LONG, DC_BURSTLEN_2LONG, DC_BURSTLEN_4LONG, DC_BURSTLEN_8LONG, DC_BURSTLEN_16LONG, DC_BURSTLEN_32LONG, DC_TXPOLL_OFF, DC_TXPOLL_1, DC_TXPOLL_2, DC_TXPOLL_3, DC_TXPOLL_4, DC_TXPOLL_5, DC_TXPOLL_6, DC_TXPOLL_7, DC_ISR_TX_OK, DC_ISR_TX_IDLE, DC_ISR_TX_NOBUF, DC_ISR_TX_JABBERTIMEO, DC_ISR_LINKGOOD, DC_ISR_TX_UNDERRUN, DC_ISR_RX_OK, DC_ISR_RX_NOBUF, DC_ISR_RX_READ, DC_ISR_RX_WATDOGTIMEO, DC_ISR_TX_EARLY, DC_ISR_TIMER_EXPIRED, DC_ISR_LINKFAIL, DC_ISR_BUS_ERR, DC_ISR_RX_EARLY, DC_ISR_ABNORMAL, DC_ISR_NORMAL, DC_ISR_RX_STATE, DC_ISR_TX_STATE, DC_ISR_BUSERRTYPE, DC_ISR_100MBPSLINK, DC_ISR_MAGICKPACK, DC_RXSTATE_STOPPED, DC_RXSTATE_FETCH, DC_RXSTATE_ENDCHECK, DC_RXSTATE_WAIT, DC_RXSTATE_SUSPEND, DC_RXSTATE_CLOSE, DC_RXSTATE_FLUSH, DC_RXSTATE_DEQUEUE, DC_TXSTATE_RESET, DC_TXSTATE_FETCH, DC_TXSTATE_WAITEND, DC_TXSTATE_READING, DC_TXSTATE_RSVD, DC_TXSTATE_SETUP, DC_TXSTATE_SUSPEND, DC_TXSTATE_CLOSE, DC_NETCFG_RX_HASHPERF, DC_NETCFG_RX_ON, DC_NETCFG_RX_HASHONLY, DC_NETCFG_RX_BADFRAMES, DC_NETCFG_RX_INVFILT, DC_NETCFG_BACKOFFCNT, DC_NETCFG_RX_PROMISC, DC_NETCFG_RX_ALLMULTI, DC_NETCFG_FULLDUPLEX, DC_NETCFG_LOOPBACK, DC_NETCFG_FORCECOLL, DC_NETCFG_TX_ON, DC_NETCFG_TX_THRESH, DC_NETCFG_TX_BACKOFF, DC_NETCFG_PORTSEL, DC_NETCFG_HEARTBEAT, DC_NETCFG_STORENFWD, DC_NETCFG_SPEEDSEL, DC_NETCFG_PCS, DC_NETCFG_SCRAMBLER, DC_NETCFG_NO_RXCRC, DC_NETCFG_RX_ALL, DC_NETCFG_CAPEFFECT, DC_OPMODE_NORM, DC_OPMODE_INTLOOP, DC_OPMODE_EXTLOOP, DC_TXTHRESH_MIN, DC_TXTHRESH_INC, DC_TXTHRESH_MAX, DC_IMR_TX_OK, DC_IMR_TX_IDLE, DC_IMR_TX_NOBUF, DC_IMR_TX_JABBERTIMEO, DC_IMR_LINKGOOD, DC_IMR_TX_UNDERRUN, DC_IMR_RX_OK, DC_IMR_RX_NOBUF, DC_IMR_RX_READ, DC_IMR_RX_WATDOGTIMEO, DC_IMR_TX_EARLY, DC_IMR_TIMER_EXPIRED, DC_IMR_LINKFAIL, DC_IMR_BUS_ERR, DC_IMR_RX_EARLY, DC_IMR_ABNORMAL, DC_IMR_NORMAL, DC_IMR_100MBPSLINK, DC_IMR_MAGICKPACK, DC_SIO_EE_CS, DC_SIO_EE_CLK, DC_SIO_EE_DATAIN, DC_SIO_EE_DATAOUT, DC_SIO_ROMDATA4, DC_SIO_ROMDATA5, DC_SIO_ROMDATA6, DC_SIO_ROMDATA7, DC_SIO_EESEL, DC_SIO_ROMSEL, DC_SIO_ROMCTL_WRITE, DC_SIO_ROMCTL_READ, DC_SIO_MII_CLK, DC_SIO_MII_DATAOUT, DC_SIO_MII_DIR, DC_SIO_MII_DATAIN, DC_EECMD_WRITE, DC_EECMD_READ, DC_EECMD_ERASE, DC_EE_NODEADDR_OFFSET, DC_EE_NODEADDR, DC_TIMER_CLKDIV, DC_TIMER_TXTIMER, DC_TIMER_TXCOUNT, DC_TIMER_RXTIMER, DC_TIMER_RXCOUNT, DC_TIMER_CONTINUOUS, DC_TIMER_VALUE, DC_TSTAT_MIIACT, DC_TSTAT_LS100, DC_TSTAT_LS10, DC_TSTAT_AUTOPOLARITY, DC_TSTAT_AUIACT, DC_TSTAT_10BTACT, DC_TSTAT_NSN, DC_TSTAT_REMFAULT, DC_TSTAT_ANEGSTAT, DC_TSTAT_LP_CAN_NWAY, DC_TSTAT_LPCODEWORD, DC_ASTAT_DISABLE, DC_ASTAT_TXDISABLE, DC_ASTAT_ABDETECT, DC_ASTAT_ACKDETECT, DC_ASTAT_CMPACKDETECT, DC_ASTAT_AUTONEGCMP, DC_ASTAT_LINKCHECK, DC_SIA_RESET, DC_SIA_AUI, DC_TCTL_ENCODER_ENB, DC_TCTL_LOOPBACK, DC_TCTL_DRIVER_ENB, DC_TCTL_LNKPULSE_ENB, DC_TCTL_HALFDUPLEX, DC_TCTL_AUTONEGENBL, DC_TCTL_RX_SQUELCH, DC_TCTL_COLL_SQUELCH, DC_TCTL_COLL_DETECT, DC_TCTL_SQE_ENB, DC_TCTL_LINKTEST, DC_TCTL_AUTOPOLARITY, DC_TCTL_SET_POL_PLUS, DC_TCTL_AUTOSENSE, DC_TCTL_100BTXHALF, DC_TCTL_100BTXFULL, DC_TCTL_100BT4, DC_WDOG_JABBERDIS, DC_WDOG_HOSTUNJAB, DC_WDOG_JABBERCLK, DC_WDOG_RXWDOGDIS, DC_WDOG_RXWDOGCLK, DC_WDOG_MUSTBEZERO, DC_WDOG_AUIBNC, DC_WDOG_ACTIVITY, DC_WDOG_LINK, DC_WDOG_CTLWREN, DC_SIAGP_RXMATCH, DC_SIAGP_INT1, DC_SIAGP_INT0, DC_SIAGP_WRITE_EN, DC_SIAGP_RXMATCH_EN, DC_SIAGP_INT1_EN, DC_SIAGP_INT0_EN, DC_SIAGP_LED3, DC_SIAGP_LED2, DC_SIAGP_LED1, DC_SIAGP_LED0, DC_SIAGP_MD_GP3_OUTPUT, DC_SIAGP_MD_GP2_OUTPUT, DC_SIAGP_MD_GP1_OUTPUT, DC_SIAGP_MD_GP0_OUTPUT, DC_SFRAME_LEN, DC_RXSTAT_FIFOOFLOW, DC_RXSTAT_CRCERR, DC_RXSTAT_DRIBBLE, DC_RXSTAT_MIIERE, DC_RXSTAT_WATCHDOG, DC_RXSTAT_FRAMETYPE, DC_RXSTAT_COLLSEEN, DC_RXSTAT_GIANT, DC_RXSTAT_LASTFRAG, DC_RXSTAT_FIRSTFRAG, DC_RXSTAT_MULTICAST, DC_RXSTAT_RUNT, DC_RXSTAT_RXTYPE, DC_RXSTAT_DE, DC_RXSTAT_RXERR, DC_RXSTAT_RXLEN, DC_RXSTAT_OWN, DC_RXCTL_BUFLEN1, DC_RXCTL_BUFLEN2, DC_RXCTL_RLINK, DC_RXCTL_RLAST, DC_TXSTAT_DEFER, DC_TXSTAT_UNDERRUN, DC_TXSTAT_LINKFAIL, DC_TXSTAT_COLLCNT, DC_TXSTAT_SQE, DC_TXSTAT_EXCESSCOLL, DC_TXSTAT_LATECOLL, DC_TXSTAT_NOCARRIER, DC_TXSTAT_CARRLOST, DC_TXSTAT_JABTIMEO, DC_TXSTAT_ERRSUM, DC_TXSTAT_OWN, DC_TXCTL_BUFLEN1, DC_TXCTL_BUFLEN2, DC_TXCTL_FILTTYPE0, DC_TXCTL_PAD, DC_TXCTL_TLINK, DC_TXCTL_TLAST, DC_TXCTL_NOCRC, DC_TXCTL_SETUP, DC_TXCTL_FILTTYPE1, DC_TXCTL_FIRSTFRAG, DC_TXCTL_LASTFRAG, DC_TXCTL_FINT, DC_FILTER_PERFECT, DC_FILTER_HASHPERF, DC_FILTER_INVERSE, DC_FILTER_HASHONLY, DC_MAXFRAGS, DC_RX_LIST_CNT, DC_TX_LIST_CNT, DC_MII_STARTDELIM, DC_MII_READOP, DC_MII_WRITEOP, DC_MII_TURNAROUND, DC_AL_CR, DC_AL_PAR0, DC_AL_PAR1, DC_AL_MAR0, DC_AL_MAR1, DC_AL_BMCR, DC_AL_BMSR, DC_AL_VENID, DC_AL_DEVID, DC_AL_ANAR, DC_AL_LPAR, DC_AL_ANER, DC_ADMTEK_PHYADDR, DC_AL_EE_NODEADDR, DC_AL_CR_ATUR, DC_AX_FILTIDX, DC_AX_FILTDATA, DC_AX_NETCFG_RX_BROAD, DC_AX_FILTIDX_PAR0, DC_AX_FILTIDX_PAR1, DC_AX_FILTIDX_MAR0, DC_AX_FILTIDX_MAR1, DC_MX_MAGICPACKET, DC_MX_NWAYSTAT, DC_MX_MPACK_DISABLE, DC_MX_NWAY_10BTHALF, DC_MX_NWAY_10BTFULL, DC_MX_NWAY_100BTHALF, DC_MX_NWAY_100BTFULL, DC_MX_NWAY_100BT4, DC_MX_MAGIC_98713, DC_MX_MAGIC_98713A, DC_MX_MAGIC_98715, DC_MX_MAGIC_98725, DC_PN_GPIO, DC_PN_PWRUP_CFG, DC_PN_SIOCTL, DC_PN_MII, DC_PN_NWAY, DC_PN_SIOCTL_DATA, DC_PN_SIOCTL_OPCODE, DC_PN_SIOCTL_BUSY, DC_PN_EEOPCODE_ERASE, DC_PN_EEOPCODE_READ, DC_PN_EEOPCODE_WRITE, DC_PN_GPIO_DATA0, DC_PN_GPIO_DATA1, DC_PN_GPIO_DATA2, DC_PN_GPIO_DATA3, DC_PN_GPIO_CTL0, DC_PN_GPIO_CTL1, DC_PN_GPIO_CTL2, DC_PN_GPIO_CTL3, DC_PN_MII_DATA, DC_PN_MII_RESERVER, DC_PN_MII_REGADDR, DC_PN_MII_PHYADDR, DC_PN_MII_OPCODE, DC_PN_MII_BUSY, DC_PN_MIIOPCODE_READ, DC_PN_MIIOPCODE_WRITE, DC_PN_NWAY_RESET, DC_PN_NWAY_PDOWN, DC_PN_NWAY_BYPASS, DC_PN_NWAY_AUILOWCUR, DC_PN_NWAY_TPEXTEND, DC_PN_NWAY_POLARITY, DC_PN_NWAY_TP, DC_PN_NWAY_AUIVOLT, DC_PN_NWAY_DUPLEX, DC_PN_NWAY_LINKTEST, DC_PN_NWAY_AUTODETECT, DC_PN_NWAY_SPEEDSEL, DC_PN_NWAY_NWAY_ENB, DC_PN_NWAY_CAP10HDX, DC_PN_NWAY_CAP10FDX, DC_PN_NWAY_CAP100FDX, DC_PN_NWAY_CAP100HDX, DC_PN_NWAY_CAP100T4, DC_PN_NWAY_ANEGRESTART, DC_PN_NWAY_REMFAULT, DC_PN_NWAY_LPAR10HDX, DC_PN_NWAY_LPAR10FDX, DC_PN_NWAY_LPAR100FDX, DC_PN_NWAY_LPAR100HDX, DC_PN_NWAY_LPAR100T4, DC_CONEXANT_PHYADDR, DC_CONEXANT_EE_NODEADDR, DC_TX_POLL, DC_TX_COALESCE, DC_TX_ADMTEK_WAR, DC_TX_USE_TX_INTR, DC_RX_FILTER_TULIP, DC_TX_INTR_FIRSTFRAG, DC_PNIC_RX_BUG_WAR, DC_TX_FIXED_RING, DC_TX_STORENFWD, DC_REDUCED_MII_POLL, DC_TX_INTR_ALWAYS, DC_21143_NWAY, DC_128BIT_HASH, DC_64BIT_HASH, DC_TULIP_LEDS, DC_TX_ONE, DC_TX_ALIGN, DC_MOMENCO_BOTCH, DC_TIMEOUT, DC_REVISION_98713, DC_REVISION_98713A, DC_REVISION_98715, DC_REVISION_98715AEC_C, DC_REVISION_98725, DC_REVISION_82C168, DC_REVISION_82C169, DC_REVISION_88140, DC_REVISION_88141, DC_REVISION_DM9102, DC_REVISION_DM9102A, DC_PCI_CFID, DC_PCI_CFCS, DC_PCI_CFRV, DC_PCI_CFLT, DC_PCI_CFBIO, DC_PCI_CFBMA, DC_PCI_CCIS, DC_PCI_CSID, DC_PCI_CBER, DC_PCI_CCAP, DC_PCI_CFIT, DC_PCI_CFDD, DC_PCI_CWUA0, DC_PCI_CWUA1, DC_PCI_SOP0, DC_PCI_SOP1, DC_PCI_CWUC, DC_PCI_CCID, DC_PCI_CPMC, DC_CFID_VENDOR, DC_CFID_DEVICE, DC_CFCS_IOSPACE, DC_CFCS_MEMSPACE, DC_CFCS_BUSMASTER, DC_CFCS_MWI_ENB, DC_CFCS_PARITYERR_ENB, DC_CFCS_SYSERR_ENB, DC_CFCS_NEWCAPS, DC_CFCS_FAST_B2B, DC_CFCS_DATAPARITY, DC_CFCS_DEVSELTIM, DC_CFCS_TGTABRT, DC_CFCS_MASTERABRT, DC_CFCS_SYSERR, DC_CFCS_PARITYERR, DC_CFRV_STEPPING, DC_CFRV_REVISION, DC_CFRV_SUBCLASS, DC_CFRV_BASECLASS, DC_21143_PB_REV, DC_21143_TB_REV, DC_21143_PC_REV, DC_21143_TC_REV, DC_21143_PD_REV, DC_21143_TD_REV, DC_CFLT_CACHELINESIZE, DC_CFLT_LATENCYTIMER, DC_CSID_VENDOR, DC_CSID_DEVICE, DC_CCAP_OFFSET, DC_CFIT_INTLINE, DC_CFIT_INTPIN, DC_CFIT_MIN_GNT, DC_CFIT_MAX_LAT, DC_CCID_CAPID, DC_CCID_NEXTPTR, DC_CCID_PM_VERS, DC_CCID_PME_CLK, DC_CCID_DVSPEC_INT, DC_CCID_STATE_D1, DC_CCID_STATE_D2, DC_CCID_PME_D0, DC_CCID_PME_D1, DC_CCID_PME_D2, DC_CCID_PME_D3HOT, DC_CCID_PME_D3COLD, DC_CPMC_STATE, DC_CPMC_PME_ENB, DC_CPMC_PME_STS, DC_PSTATE_D0, DC_PSTATE_D1, DC_PSTATE_D2, DC_PSTATE_D3, DC_CFDD_DRVUSE, DC_CFDD_SNOOZE_MODE, DC_CFDD_SLEEP_MODE, DC_CWUC_MUST_BE_ZERO, DC_CWUC_SECUREON_ENB, DC_CWUC_FORCE_WUL, DC_CWUC_BNC_ABILITY, DC_CWUC_AUI_ABILITY, DC_CWUC_TP10_ABILITY, DC_CWUC_MII_ABILITY, DC_CWUC_SYM_ABILITY, DC_CWUC_LOCK, DC_IB_CTLRCNT, DC_IB_LEAF0_CNUM, DC_IB_LEAF0_OFFSET, DC_CTYPE_10BT, DC_CTYPE_10BT_NWAY, DC_CTYPE_10BT_FDX, DC_CTYPE_10B2, DC_CTYPE_10B5, DC_CTYPE_100BT, DC_CTYPE_100BT_FDX, DC_CTYPE_100T4, DC_CTYPE_100FX, DC_CTYPE_100FX_FDX, DC_CTYPE_MII_10BT, DC_CTYPE_MII_10BT_FDX, DC_CTYPE_MII_100BT, DC_CTYPE_MII_100BT_FDX, DC_CTYPE_MII_100T4, DC_CTYPE_MII_100FX, DC_CTYPE_MII_100FX_FDX, DC_CTYPE_DYN_PUP_AUTOSENSE, DC_CTYPE_PUP_AUTOSENSE, DC_CTYPE_NOMEDIA, DC_EBLOCK_SIA, DC_EBLOCK_MII, DC_EBLOCK_SYM, DC_EBLOCK_RESET, DC_EBLOCK_PHY_SHUTDOWN, DC_SIA_CODE_10BT, DC_SIA_CODE_10B2, DC_SIA_CODE_10B5, DC_SIA_CODE_10BT_FDX, DC_SIA_CODE_EXT, DC_SYM_CODE_100BT, DC_SYM_CODE_100BT_FDX, DC_SYM_CODE_100T4, DC_SYM_CODE_100FX, DC_SYM_CODE_100FX_FDX, DC_RXSTAT, DC_PN_GPIO_SPEEDSEL, DC_PN_GPIO_100TX_LOOP, DC_PN_GPIO_BNC_ENB, DC_PN_GPIO_100TX_LNK);
        // DC_INTRS spans continuation lines the parser does not join (`intrs` checks it); the
        // `#if 0` thresholds are not compiled in C either.
        ours.extend([
            "DC_INTRS",
            "DC_TXTHRESH_72BYTES",
            "DC_TXTHRESH_96BYTES",
            "DC_TXTHRESH_128BYTES",
            "DC_TXTHRESH_160BYTES",
        ]);
        crate::reftest::assert_complete(&defs, "DC_", &ours);
    }

    #[test]
    fn intrs() {
        assert_eq!(DC_INTRS, 0x0001_a2e7);
    }

    #[test]
    fn list_layout() {
        assert_eq!(size_of::<DcListData>(), 64 * 32 + 256 * 32 + 192 + 64);
        assert_eq!(dc_rx_list_off(1), 32);
        assert_eq!(dc_tx_list_off(0), 64 * 32);
        assert_eq!(DC_SBUF_OFF, 320 * 32);
        assert_eq!(DcDescWord::Next.offset(), 12);
        assert_eq!(
            offset_of!(DcEblockSia, dc_un) + offset_of!(DcSiaExt, dc_sia_gpio_ctl),
            9
        );
        assert_eq!(size_of::<DcEblockSym>(), 9);
    }

    #[test]
    fn macros() {
        assert_eq!(dc_rxbytes(0x05ea_0300), 0x05ea);
        assert_eq!(dc_inc(DC_TX_LIST_CNT - 1, DC_TX_LIST_CNT), 0);
        assert_eq!(dc_inc(3, DC_RX_LIST_CNT), 4);
        let ea = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        assert_eq!(dc_sp_field(&ea, 0), 0x5452);
        assert_eq!(dc_sp_field(&ea, 2), 0x5634);
    }
}
/* </TESTS> */
