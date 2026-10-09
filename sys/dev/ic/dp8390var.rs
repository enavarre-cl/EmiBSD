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
 * Device driver for National Semiconductor DS8390/WD83C690 based ethernet
 * adapters.
 *
 * Copyright (c) 1994, 1995 Charles M. Hannum.  All rights reserved.
 *
 * Copyright (C) 1993, David Greenman.  This software may be used, modified,
 * copied, distributed, and sold, in both source and binary form provided that
 * the above copyright and these terms are retained.  Under no circumstances is
 * the author responsible for the proper functioning of this software, nor does
 * the author assume any responsibility for damages incurred with its use.
 */
/* </LICENSES> */

/* <CODE> */
//! National Semiconductor DS8390/WD83C690 driver softc (`dev/ic/dp8390var.h`).
//!
//! Upstream: sys/dev/ic/dp8390var.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the [`Device`] first and every member a `Cell` (it is
//!   zeroed softc memory); the bus space tag and handles are `Option`s, the callbacks
//!   `Option`s of `fn` pointers (all-zero valid). `struct mii_data sc_mii` is kept for its
//!   `mii_media`; `sc_media` of the C is [`Dp8390Softc::sc_media`].
//! - The callbacks take `&'static Dp8390Softc`. `ring_copy` takes the destination as a slice
//!   (its length is the C's `amount`), `read_hdr` fills a [`Dp8390Ring`], and the media
//!   change callback returns `Result<(), Errno>`.
//! - `NIC_GET`, `NIC_PUT` and `NIC_BARRIER` are the methods [`Dp8390Softc::nic_get`],
//!   [`Dp8390Softc::nic_put`] and [`Dp8390Softc::nic_barrier`]; the register map is
//!   `sc_reg_map`.

use core::cell::Cell;
use core::ffi::c_void;

use crate::dev::ic::dp8390reg::Dp8390Ring;
use crate::dev::mii::miivar::MiiData;
use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BUS_SPACE_BARRIER_READ, BUS_SPACE_BARRIER_WRITE, BusSize, BusSpaceHandle, BusSpaceTag,
    bus_space_barrier, bus_space_read_1, bus_space_write_1,
};
use crate::net::if_::Ifmediareq;
use crate::net::if_media::Ifmedia;
use crate::net::if_var::Ifnet;
use crate::netinet::if_ether::Arpcom;
use crate::sys::device::{Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::mbuf::Mbuf;

/// `INTERFACE_NAME_LEN`.
pub const INTERFACE_NAME_LEN: usize = 32;

/// The callbacks of [`Dp8390Softc`] that take the softc alone and return nothing
/// (`init_card`, `stop_card`, `recv_int`, `sc_disable`, `sc_media_init`, `sc_media_fini`).
pub type Dp8390Fn = fn(&'static Dp8390Softc);
/// `test_mem` and `sc_enable`: take the softc and return an `int`.
pub type Dp8390IntFn = fn(&'static Dp8390Softc) -> i32;
/// `read_hdr`: the softc, a NIC memory offset and the header to fill.
pub type Dp8390ReadHdrFn = fn(&'static Dp8390Softc, i32, &mut Dp8390Ring);
/// `ring_copy`: the softc, a NIC memory offset and the destination; returns the offset after.
pub type Dp8390RingCopyFn = fn(&'static Dp8390Softc, i32, &mut [u8]) -> i32;
/// `write_mbuf`: the softc, the chain and a NIC memory offset; returns the length written.
pub type Dp8390WriteMbufFn = fn(&'static Dp8390Softc, &Mbuf, i32) -> i32;
/// `sc_mediachange`.
pub type Dp8390MediaChangeFn = fn(&'static Dp8390Softc) -> Result<(), Errno>;
/// `sc_mediastatus`.
pub type Dp8390MediaStatusFn = fn(&'static Dp8390Softc, &mut Ifmediareq);

/// `struct dp8390_softc`: per line info and status.
#[repr(C)]
pub struct Dp8390Softc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_ih`: interrupt handler.
    pub sc_ih: Cell<*mut c_void>,
    /// `sc_flags`: interface flags, from config (`DP8390_*`).
    pub sc_flags: Cell<i32>,

    /// `sc_arpcom`: ethernet common.
    pub sc_arpcom: Arpcom,
    /// `sc_mii`: MII glue; `sc_media` is its `mii_media`.
    pub sc_mii: MiiData,

    /// `sc_regt`: NIC register space tag.
    pub sc_regt: Cell<Option<BusSpaceTag>>,
    /// `sc_regh`: NIC register space handle.
    pub sc_regh: Cell<Option<BusSpaceHandle>>,
    /// `sc_buft`: buffer space tag.
    pub sc_buft: Cell<Option<BusSpaceTag>>,
    /// `sc_bufh`: buffer space handle.
    pub sc_bufh: Cell<Option<BusSpaceHandle>>,

    /// `sc_reg_map`: register map (offsets).
    pub sc_reg_map: [Cell<BusSize>; 16],

    /// `is790`: NIC is a 790.
    pub is790: Cell<i32>,

    /// `cr_proto`: values always set in CR.
    pub cr_proto: Cell<u8>,
    /// `rcr_proto`: values always set in RCR.
    pub rcr_proto: Cell<u8>,
    /// `dcr_reg`: override DCR iff LS is set.
    pub dcr_reg: Cell<u8>,

    /// `mem_start`: offset of NIC memory.
    pub mem_start: Cell<i32>,
    /// `mem_end`: offset of NIC memory end.
    pub mem_end: Cell<i32>,
    /// `mem_size`: total shared memory size.
    pub mem_size: Cell<i32>,
    /// `mem_ring`: offset of start of RX ring-buffer.
    pub mem_ring: Cell<i32>,

    /// `txb_cnt`: number of transmit buffers.
    pub txb_cnt: Cell<u16>,
    /// `txb_inuse`: number of transmit buffers active.
    pub txb_inuse: Cell<u16>,

    /// `txb_new`: pointer to where new buffer will be added.
    pub txb_new: Cell<u16>,
    /// `txb_next_tx`: pointer to next buffer ready to xmit.
    pub txb_next_tx: Cell<u16>,
    /// `txb_len`: buffered xmit buffer lengths.
    pub txb_len: [Cell<u16>; 8],
    /// `tx_page_start`: first page of TX buffer area.
    pub tx_page_start: Cell<u16>,
    /// `rec_page_start`: first page of RX ring-buffer.
    pub rec_page_start: Cell<u16>,
    /// `rec_page_stop`: last page of RX ring-buffer.
    pub rec_page_stop: Cell<u16>,
    /// `next_packet`: pointer to next unread RX packet.
    pub next_packet: Cell<u16>,

    /// `sc_enabled`: boolean; power enabled on interface.
    pub sc_enabled: Cell<i32>,

    /// `test_mem`.
    pub test_mem: Cell<Option<Dp8390IntFn>>,
    /// `init_card`.
    pub init_card: Cell<Option<Dp8390Fn>>,
    /// `stop_card`.
    pub stop_card: Cell<Option<Dp8390Fn>>,
    /// `read_hdr`: reads the ring header at the given NIC memory offset.
    pub read_hdr: Cell<Option<Dp8390ReadHdrFn>>,
    /// `recv_int`.
    pub recv_int: Cell<Option<Dp8390Fn>>,
    /// `ring_copy`: copies `dst.len()` bytes from the ring at the given offset; returns the
    /// offset after them.
    pub ring_copy: Cell<Option<Dp8390RingCopyFn>>,
    /// `write_mbuf`: writes the chain to the given NIC memory offset; returns its length.
    pub write_mbuf: Cell<Option<Dp8390WriteMbufFn>>,

    /// `sc_enable`.
    pub sc_enable: Cell<Option<Dp8390IntFn>>,
    /// `sc_disable`.
    pub sc_disable: Cell<Option<Dp8390Fn>>,

    /// `sc_media_init`.
    pub sc_media_init: Cell<Option<Dp8390Fn>>,
    /// `sc_media_fini`.
    pub sc_media_fini: Cell<Option<Dp8390Fn>>,

    /// `sc_mediachange`.
    pub sc_mediachange: Cell<Option<Dp8390MediaChangeFn>>,
    /// `sc_mediastatus`.
    pub sc_mediastatus: Cell<Option<Dp8390MediaStatusFn>>,
}

// SAFETY: `#[repr(C)]` with the `Device` first; every other member is a `Cell` of an integer,
// raw pointer, `Option` of a tag/handle/`fn` (null-niche, all-zero valid) or the zero-valid
// arpcom/mii structures, as the other drivers' softcs.
unsafe impl Softc for Dp8390Softc {}

impl Dp8390Softc {
    /// `sc->sc_dev.dv_xname`.
    pub fn devname(&self) -> &str {
        self.sc_dev.xname()
    }

    /// `&sc->sc_arpcom.ac_if`.
    pub fn ifp(&'static self) -> &'static Ifnet {
        &self.sc_arpcom.ac_if
    }

    /// `&sc->sc_media` (`sc_mii.mii_media`).
    pub fn sc_media(&self) -> &Ifmedia {
        &self.sc_mii.mii_media
    }

    /// `sc->sc_regt` and `sc->sc_regh`, which the bus front end sets before anything else.
    pub fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.sc_regt.get(), self.sc_regh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("{}: registers not mapped", self.devname())),
        }
    }

    /// `sc->sc_buft` and `sc->sc_bufh`.
    pub fn bufs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.sc_buft.get(), self.sc_bufh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("{}: buffer space not mapped", self.devname())),
        }
    }

    /// `NIC_GET(sc->sc_regt, sc->sc_regh, reg)`.
    pub fn nic_get(&self, reg: usize) -> u8 {
        let (t, h) = self.regs();
        bus_space_read_1(t, h, self.sc_reg_map[reg].get())
    }

    /// `NIC_PUT(sc->sc_regt, sc->sc_regh, reg, val)`.
    pub fn nic_put(&self, reg: usize, val: u8) {
        let (t, h) = self.regs();
        bus_space_write_1(t, h, self.sc_reg_map[reg].get(), val)
    }

    /// `NIC_BARRIER(sc->sc_regt, sc->sc_regh)`.
    pub fn nic_barrier(&self) {
        let (t, h) = self.regs();
        bus_space_barrier(
            t,
            h,
            0,
            0x10,
            BUS_SPACE_BARRIER_READ | BUS_SPACE_BARRIER_WRITE,
        )
    }
}

/// `DP8390_VENDOR_UNKNOWN`: unknown network card.
pub const DP8390_VENDOR_UNKNOWN: i32 = 0xff;
/// `DP8390_VENDOR_WD_SMC`: Western Digital/SMC.
pub const DP8390_VENDOR_WD_SMC: i32 = 0x00;
/// `DP8390_VENDOR_3COM`: 3Com.
pub const DP8390_VENDOR_3COM: i32 = 0x01;
/// `DP8390_VENDOR_NOVELL`: Novell.
pub const DP8390_VENDOR_NOVELL: i32 = 0x02;
/// `DP8390_VENDOR_APPLE`: Apple Ethernet card.
pub const DP8390_VENDOR_APPLE: i32 = 0x10;
/// `DP8390_VENDOR_INTERLAN`: Interlan A310 card (GatorCard).
pub const DP8390_VENDOR_INTERLAN: i32 = 0x11;
/// `DP8390_VENDOR_DAYNA`: DaynaPORT E/30s (and others?).
pub const DP8390_VENDOR_DAYNA: i32 = 0x12;
/// `DP8390_VENDOR_ASANTE`: Asante MacCon II/E.
pub const DP8390_VENDOR_ASANTE: i32 = 0x13;
/// `DP8390_VENDOR_FARALLON`: Farallon EtherMac II-TP.
pub const DP8390_VENDOR_FARALLON: i32 = 0x14;
/// `DP8390_VENDOR_FOCUS`: FOCUS Enhancements EtherLAN.
pub const DP8390_VENDOR_FOCUS: i32 = 0x15;
/// `DP8390_VENDOR_KINETICS`: Kinetics EtherPort SE/30.
pub const DP8390_VENDOR_KINETICS: i32 = 0x16;
/// `DP8390_VENDOR_CABLETRON`: Cabletron Ethernet.
pub const DP8390_VENDOR_CABLETRON: i32 = 0x17;

/// `DP8390_DISABLE_TRANSCEIVER`: the default for enabling/disabling the transceiver.
pub const DP8390_DISABLE_TRANSCEIVER: i32 = 0x0001;
/// `DP8390_FORCE_8BIT_MODE`: forces the board to be used in 8-bit mode even if it
/// autoconfigs differently.
pub const DP8390_FORCE_8BIT_MODE: i32 = 0x0002;
/// `DP8390_FORCE_16BIT_MODE`: as above, 16-bit mode.
pub const DP8390_FORCE_16BIT_MODE: i32 = 0x0004;
/// `DP8390_NO_MULTI_BUFFERING`: disables the use of multiple transmit buffers.
pub const DP8390_NO_MULTI_BUFFERING: i32 = 0x0008;
/// `DP8390_FORCE_PIO`: forces all operations with the NIC memory to use Programmed I/O (i.e.
/// not via shared memory).
pub const DP8390_FORCE_PIO: i32 = 0x0010;
/// `DP8390_DO_AX88190_WORKAROUND`: the chip is ASIX AX88190 and needs work around.
pub const DP8390_DO_AX88190_WORKAROUND: i32 = 0x0020;
/// `DP8390_ATTACHED`: attach has succeeded.
pub const DP8390_ATTACHED: i32 = 0x0040;
/// `DP8390_NO_REMOTE_DMA_COMPLETE`: ASIX AX88796 doesn't have remote DMA complete bit in ISR,
/// so don't check ISR.RDC.
pub const DP8390_NO_REMOTE_DMA_COMPLETE: i32 = 0x0080;
/* </CODE> */
