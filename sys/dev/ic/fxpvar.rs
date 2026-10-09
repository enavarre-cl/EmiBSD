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
 * Copyright (c) 1995, David Greenman
 * All rights reserved.
 *
 * Modifications to support NetBSD:
 * Copyright (c) 1997 Jason R. Thorpe.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice unmodified, this list of conditions, and the following
 *    disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *      Id: if_fxpvar.h,v 1.6 1998/08/02 00:29:15 dg Exp
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/ic/fxpvar.h>`: the softc of fxp(4), the Intel EtherExpress PRO/100 driver, and the
//! helpers the C defines as macros (`CSR_READ_2`, `FXP_RXMAP_GET`, `FXP_TXCB_SYNC`, ...).
//!
//! Upstream: sys/dev/ic/fxpvar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct fxp_txsw` keeps the mbuf and the DMA map only. `tx_next` is the next index modulo
//!   `FXP_NTXCB`, `tx_cb` is `sc_ctrl->tx_cb[i]` and `tx_off` is `offsetof(struct fxp_ctrl,
//!   tx_cb[i])`, all computed from the slot's index; `sc_cbt_cons`, `sc_cbt_prod` and
//!   `sc_cbt_prev` are such indexes instead of pointers into `txs[]`.
//! - `sc_ctrl` is a raw pointer to the `bus_dmamem_map`ped control structure; `fxp_ctrl` is a
//!   `#[repr(C)]` struct with the C's layout and its `u` union is a Rust `union`. The helpers
//!   below read and write it with volatile accesses.
//! - `cbl_base` (the base of the TxCB list) is never used by the C and is not kept.
//! - `sc_ucodebuf` is the `M_DEVBUF` buffer `loadfirmware` returned, as a byte pointer
//!   (`sc_ucodelen` bytes); the dwords are read from it unaligned-safe.
//! - The macros become methods of [`FxpSoftc`]; the macro arguments `p` are the
//!   `BUS_DMASYNC_*` flags.

use core::cell::Cell;
use core::mem::offset_of;
use core::ptr;

use crate::dev::ic::fxpreg::{
    FXP_NTXSEG, FxpCbConfig, FxpCbIas, FxpCbMcs, FxpCbTx, FxpCbUcode, FxpStats,
};
use crate::dev::mii::miivar::MiiData;
use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BusAddr, BusDmaSegment, BusDmaTag, BusDmamap, BusSize, BusSpaceHandle, BusSpaceTag,
    bus_dmamap_sync, bus_space_read_2, bus_space_read_4, bus_space_write_2, bus_space_write_4,
};
use crate::netinet::if_ether::Arpcom;
use crate::sys::device::{Device, Softc};
use crate::sys::mbuf::Mbuf;
use crate::sys::timeout::Timeout;

/// `FXP_NTXCB`: number of transmit control blocks. This determines the number of transmit
/// buffers that can be chained in the CB list. This must be a power of two.
pub const FXP_NTXCB: usize = 128;

/// `FXP_NRFABUFS_MIN`: minimum number of receive frame area buffers.
pub const FXP_NRFABUFS_MIN: usize = 4;
/// `FXP_NRFABUFS_MAX`: maximum number of receive frame area buffers. These are large so
/// choose wisely.
pub const FXP_NRFABUFS_MAX: usize = 64;

/// `FXP_INT_DELAY`: default maximum time, in microseconds, that an interrupt may be delayed
/// in an attempt to coalesce interrupts. Only effective if the Intel microcode is loaded.
pub const FXP_INT_DELAY: i32 = 128;

/// `FXP_BUNDLE_MAX`: default number of packets that will be bundled before an interrupt is
/// generated. Only effective if the Intel microcode is loaded; not present in all microcode
/// revisions.
pub const FXP_BUNDLE_MAX: i32 = 16;

/// `FXP_MIN_SIZE_MASK`: bit-mask describing the minimum size frame that will be bundled.
/// Only effective if the Intel microcode is loaded; not present in all microcode revisions.
/// Disabled by default, to reduce receiving immediately interrupts from all frames with size
/// less than 128 bytes.
pub const FXP_MIN_SIZE_MASK: i32 = 0xFFFF;

/// A bounds-checked, alignment-checked, volatile view of DMA memory the chip shares with the
/// driver (the control structure, or the receive frame area at the start of a cluster). The C
/// reads and writes `volatile` structure members through pointers; every access here is one
/// volatile load or store of an integer (or of a whole `Copy` block) at an offset.
#[derive(Clone, Copy)]
pub struct Dma {
    /// The first byte.
    base: *mut u8,
    /// The number of bytes the view covers.
    len: usize,
}

impl Dma {
    /// A view of `len` bytes at `base`.
    ///
    /// # Safety
    ///
    /// `base` is valid for volatile reads and writes of `len` bytes for as long as the view
    /// (or a copy of it) is used.
    pub unsafe fn new(base: *mut u8, len: usize) -> Self {
        Self { base, len }
    }

    /// The address of `size` bytes at `off`, aligned to `align`.
    fn at(&self, off: usize, size: usize, align: usize) -> *mut u8 {
        if off.checked_add(size).is_none_or(|end| end > self.len) {
            panic(format_args!(
                "fxp: DMA access {off}+{size} past {}",
                self.len
            ));
        }
        let p = self.base.wrapping_add(off);
        if !(p as usize).is_multiple_of(align) {
            panic(format_args!("fxp: misaligned DMA access at {p:p}"));
        }
        p
    }

    /// A volatile 16-bit load at `off`.
    pub fn rd16(&self, off: usize) -> u16 {
        // SAFETY: `at` checked the range (inside the view) and the alignment.
        unsafe { ptr::read_volatile(self.at(off, 2, 2).cast::<u16>()) }
    }

    /// A volatile 16-bit store at `off`.
    pub fn wr16(&self, off: usize, v: u16) {
        // SAFETY: as for `rd16`.
        unsafe { ptr::write_volatile(self.at(off, 2, 2).cast::<u16>(), v) }
    }

    /// A volatile 32-bit load at `off`.
    pub fn rd32(&self, off: usize) -> u32 {
        // SAFETY: `at` checked the range and the alignment.
        unsafe { ptr::read_volatile(self.at(off, 4, 4).cast::<u32>()) }
    }

    /// A volatile 32-bit store at `off`.
    pub fn wr32(&self, off: usize, v: u32) {
        // SAFETY: as for `rd32`.
        unsafe { ptr::write_volatile(self.at(off, 4, 4).cast::<u32>(), v) }
    }

    /// A volatile 8-bit store at `off`.
    pub fn wr8(&self, off: usize, v: u8) {
        // SAFETY: `at` checked the range; a byte has no alignment.
        unsafe { ptr::write_volatile(self.at(off, 1, 1), v) }
    }

    /// A volatile load of the plain-data block `T` at `off`.
    pub fn read<T: Copy>(&self, off: usize) -> T {
        // SAFETY: `at` checked the range and `T`'s alignment; `T: Copy` is plain data the
        // callers instantiate with the `#[repr(C)]` blocks of `fxpreg.rs` (any bit pattern
        // valid).
        unsafe { ptr::read_volatile(self.at(off, size_of::<T>(), align_of::<T>()).cast::<T>()) }
    }

    /// A volatile store of the plain-data block `T` at `off`.
    pub fn write<T: Copy>(&self, off: usize, v: T) {
        // SAFETY: as for `read`.
        unsafe { ptr::write_volatile(self.at(off, size_of::<T>(), align_of::<T>()).cast::<T>(), v) }
    }
}

/// `struct fxp_txsw`: the software state of one transmit control block.
#[repr(C)]
pub struct FxpTxsw {
    /// `tx_mbuf`.
    pub tx_mbuf: Cell<Option<&'static Mbuf>>,
    /// `tx_map`.
    pub tx_map: Cell<Option<&'static BusDmamap>>,
}

/// `union { mcs; ias; cfg; code; } u` of `struct fxp_ctrl`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union FxpCtrlU {
    /// `mcs`.
    pub mcs: FxpCbMcs,
    /// `ias`.
    pub ias: FxpCbIas,
    /// `cfg`.
    pub cfg: FxpCbConfig,
    /// `code`.
    pub code: FxpCbUcode,
}

/// `struct fxp_ctrl`: the control structures the chip reads and writes, in one DMA segment.
#[repr(C)]
pub struct FxpCtrl {
    /// `tx_cb`.
    pub tx_cb: [FxpCbTx; FXP_NTXCB],
    /// `stats`.
    pub stats: FxpStats,
    /// `u`.
    pub u: FxpCtrlU,
}

/// `FXPF_MWI_ENABLE`: enable use of PCI MWI command.
pub const FXPF_MWI_ENABLE: i32 = 0x10;
/// `FXPF_DISABLE_STANDBY`: currently need to work-around.
pub const FXPF_DISABLE_STANDBY: i32 = 0x20;
/// `FXPF_UCODELOADED`: ucode load already attempted.
pub const FXPF_UCODELOADED: i32 = 0x40;
/// `FXPF_NOUCODE`: no ucode for this chip.
pub const FXPF_NOUCODE: i32 = 0x80;
/// `FXPF_RECV_WORKAROUND`: receiver lock-up workaround.
pub const FXPF_RECV_WORKAROUND: i32 = 0x100;

/// `struct fxp_softc`.
#[repr(C)]
pub struct FxpSoftc {
    /// `sc_dev`: generic device structures.
    pub sc_dev: Device,
    /// `sc_ih`: interrupt handler cookie.
    pub sc_ih: Cell<*mut core::ffi::c_void>,
    /// `sc_st`: bus space tag.
    pub sc_st: Cell<Option<BusSpaceTag>>,
    /// `sc_sh`: bus space handle.
    pub sc_sh: Cell<Option<BusSpaceHandle>>,
    /// `sc_dmat`: bus dma tag.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_arpcom`: per-interface network data.
    pub sc_arpcom: Arpcom,
    /// `sc_mii`: MII media information.
    pub sc_mii: MiiData,
    /// `rfa_headm`: first mbuf in receive frame area.
    pub rfa_headm: Cell<Option<&'static Mbuf>>,
    /// `rfa_tailm`: last mbuf in receive frame area.
    pub rfa_tailm: Cell<Option<&'static Mbuf>>,
    /// `sc_flags`: misc. flags (`FXPF_*`).
    pub sc_flags: Cell<i32>,
    /// `stats_update_to`: timeout of the statistics update.
    pub stats_update_to: Timeout,
    /// `rx_idle_secs`: number of seconds RX has been idle.
    pub rx_idle_secs: Cell<i32>,
    /// `phy_primary_addr`: address of primary PHY.
    pub phy_primary_addr: Cell<i32>,
    /// `phy_primary_device`: device type of primary PHY.
    pub phy_primary_device: Cell<i32>,
    /// `phy_10Mbps_only`: PHY is 10Mbps-only device.
    pub phy_10mbps_only: Cell<i32>,
    /// `eeprom_size`: size of serial EEPROM.
    pub eeprom_size: Cell<i32>,
    /// `rx_bufs`: how many rx buffers allocated?
    pub rx_bufs: Cell<i32>,
    /// `txs`.
    pub txs: [FxpTxsw; FXP_NTXCB],
    /// `sc_cbt_cons`: index into `txs`.
    pub sc_cbt_cons: Cell<usize>,
    /// `sc_cbt_prod`: index into `txs`.
    pub sc_cbt_prod: Cell<usize>,
    /// `sc_cbt_prev`: index into `txs`.
    pub sc_cbt_prev: Cell<usize>,
    /// `sc_cbt_cnt`.
    pub sc_cbt_cnt: Cell<i32>,
    /// `tx_cb_map`: the map of `sc_ctrl`.
    pub tx_cb_map: Cell<Option<&'static BusDmamap>>,
    /// `sc_cb_seg`.
    pub sc_cb_seg: Cell<BusDmaSegment>,
    /// `sc_cb_nseg`.
    pub sc_cb_nseg: Cell<i32>,
    /// `sc_ctrl`: the control structures, mapped.
    pub sc_ctrl: Cell<*mut FxpCtrl>,
    /// `sc_rxmaps`.
    pub sc_rxmaps: [Cell<Option<&'static BusDmamap>>; FXP_NRFABUFS_MAX],
    /// `sc_rxfree`.
    pub sc_rxfree: Cell<usize>,
    /// `sc_revision`: chip revision.
    pub sc_revision: Cell<u32>,
    /// `sc_int_delay`: interrupt delay value for ucode.
    pub sc_int_delay: Cell<u16>,
    /// `sc_bundle_max`: max # frames per interrupt (ucode).
    pub sc_bundle_max: Cell<u16>,
    /// `sc_min_size_mask`: bit-mask describing the minimum size of frame that will be
    /// bundled.
    pub sc_min_size_mask: Cell<u16>,
    /// `sc_ucodebuf`: the microcode file `loadfirmware` read (`M_DEVBUF`).
    pub sc_ucodebuf: Cell<*mut u8>,
    /// `sc_ucodelen`: its length in bytes.
    pub sc_ucodelen: Cell<usize>,
}

// SAFETY: `#[repr(C)]` with the device first; the arpcom, the MII data, the timeout, the
// segment and the arrays of `Cell`s of `Option`s of references are all-zero valid, and every
// other member is a `Cell` of an integer or a pointer.
unsafe impl Softc for FxpSoftc {}

impl FxpSoftc {
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

    /// `sc->sc_st` and `sc->sc_sh`, which the bus front-end maps first.
    pub fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.sc_st.get(), self.sc_sh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("{}: registers not mapped", self.devname())),
        }
    }

    /// `CSR_READ_2(sc, reg)`.
    pub fn csr_read_2(&self, reg: usize) -> u16 {
        let (t, h) = self.regs();
        bus_space_read_2(t, h, reg as BusSize)
    }

    /// `CSR_READ_4(sc, reg)`.
    pub fn csr_read_4(&self, reg: usize) -> u32 {
        let (t, h) = self.regs();
        bus_space_read_4(t, h, reg as BusSize)
    }

    /// `CSR_WRITE_2(sc, reg, val)`.
    pub fn csr_write_2(&self, reg: usize, val: u16) {
        let (t, h) = self.regs();
        bus_space_write_2(t, h, reg as BusSize, val);
    }

    /// `CSR_WRITE_4(sc, reg, val)`.
    pub fn csr_write_4(&self, reg: usize, val: u32) {
        let (t, h) = self.regs();
        bus_space_write_4(t, h, reg as BusSize, val);
    }

    /// `sc->tx_cb_map`, which `fxp_attach` creates and loads.
    pub fn cb_map(&self) -> &'static BusDmamap {
        match self.tx_cb_map.get() {
            Some(m) => m,
            None => panic(format_args!("{}: no control map", self.devname())),
        }
    }

    /// `sc->tx_cb_map->dm_segs->ds_addr`: the bus address of the control structure.
    pub fn cb_base(&self) -> BusAddr {
        self.cb_map().dm_segs()[0].get().ds_addr
    }

    /// `sc->sc_ctrl`, as a bounds-checked view of the mapped control structure.
    pub fn ctrl_dma(&self) -> Dma {
        let p = self.sc_ctrl.get();
        if p.is_null() {
            panic(format_args!(
                "{}: control structure not mapped",
                self.devname()
            ));
        }
        // SAFETY: `fxp_attach` mapped `size_of::<FxpCtrl>()` bytes of DMA memory at
        // `sc_ctrl` (page aligned) and clears the pointer before unmapping them; the memory
        // is the chip's as well, so every access is volatile.
        unsafe { Dma::new(p.cast::<u8>(), size_of::<FxpCtrl>()) }
    }

    /// `FXP_RXMAP_GET(sc)`: `sc_rxmaps[sc_rxfree++]`.
    pub fn rxmap_get(&self) -> &'static BusDmamap {
        let i = self.sc_rxfree.get();
        self.sc_rxfree.set(i + 1);
        match self.sc_rxmaps.get(i).and_then(Cell::get) {
            Some(m) => m,
            None => panic(format_args!("{}: no free rx map", self.devname())),
        }
    }

    /// `FXP_RXMAP_PUT(sc, map)`: `sc_rxmaps[--sc_rxfree] = map`.
    pub fn rxmap_put(&self, map: &'static BusDmamap) {
        let i = self.sc_rxfree.get();
        if i == 0 {
            panic(format_args!("{}: rx map put without get", self.devname()));
        }
        self.sc_rxfree.set(i - 1);
        self.sc_rxmaps[i - 1].set(Some(map));
    }

    /// `bus_dmamap_sync(sc->sc_dmat, sc->tx_cb_map, off, len, p)`.
    fn cb_sync(&self, off: usize, len: usize, p: i32) {
        bus_dmamap_sync(self.dmat(), self.cb_map(), off, len, p);
    }

    /// `FXP_TXCB_SYNC(sc, txs, p)` for the slot `i`.
    pub fn txcb_sync(&self, i: usize, p: i32) {
        self.cb_sync(
            offset_of!(FxpCtrl, tx_cb) + i * size_of::<FxpCbTx>(),
            size_of::<FxpCbTx>(),
            p,
        );
    }

    /// `FXP_MCS_SYNC(sc, p)`.
    pub fn mcs_sync(&self, p: i32) {
        self.cb_sync(offset_of!(FxpCtrl, u), size_of::<FxpCbMcs>(), p);
    }

    /// `FXP_IAS_SYNC(sc, p)`.
    pub fn ias_sync(&self, p: i32) {
        self.cb_sync(offset_of!(FxpCtrl, u), size_of::<FxpCbIas>(), p);
    }

    /// `FXP_CFG_SYNC(sc, p)`.
    pub fn cfg_sync(&self, p: i32) {
        self.cb_sync(offset_of!(FxpCtrl, u), size_of::<FxpCbConfig>(), p);
    }

    /// `FXP_UCODE_SYNC(sc, p)`.
    pub fn ucode_sync(&self, p: i32) {
        self.cb_sync(offset_of!(FxpCtrl, u), size_of::<FxpCbUcode>(), p);
    }

    /// `FXP_STATS_SYNC(sc, p)`.
    pub fn stats_sync(&self, p: i32) {
        self.cb_sync(offset_of!(FxpCtrl, stats), size_of::<FxpStats>(), p);
    }

    /// `bus_dmamap_sync(sc->sc_dmat, sc->tx_cb_map, 0, sc->tx_cb_map->dm_mapsize, p)`.
    pub fn cb_sync_all(&self, p: i32) {
        self.cb_sync(0, self.cb_map().dm_mapsize.get(), p);
    }
}

/// `FXP_MBUF_SYNC(sc, m, p)`: `bus_dmamap_sync(sc->sc_dmat, m, 0, m->dm_mapsize, p)`.
pub fn fxp_mbuf_sync(sc: &FxpSoftc, map: &BusDmamap, p: i32) {
    bus_dmamap_sync(sc.dmat(), map, 0, map.dm_mapsize.get(), p);
}

/// `offsetof(struct fxp_ctrl, tx_cb[i])`.
pub const fn fxp_ctrl_tx_cb_off(i: usize) -> usize {
    offset_of!(FxpCtrl, tx_cb) + i * size_of::<FxpCbTx>()
}

/// `offsetof(struct fxp_ctrl, tx_cb[i].tbd[0])`.
pub const fn fxp_ctrl_tbd_off(i: usize) -> usize {
    fxp_ctrl_tx_cb_off(i) + offset_of!(FxpCbTx, tbd)
}

/// `offsetof(struct fxp_ctrl, stats)`.
pub const FXP_CTRL_STATS_OFF: usize = offset_of!(FxpCtrl, stats);
/// `offsetof(struct fxp_ctrl, u.mcs)` (and `u.ias`, `u.cfg`, `u.code`: a union starts at
/// the same offset for every member).
pub const FXP_CTRL_U_OFF: usize = offset_of!(FxpCtrl, u);

const _: () = assert!(FXP_NTXSEG == 30 && FXP_NTXCB.is_power_of_two());
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_structure_layout() {
        assert_eq!(fxp_ctrl_tx_cb_off(0), 0);
        assert_eq!(fxp_ctrl_tx_cb_off(1), 256);
        assert_eq!(fxp_ctrl_tbd_off(2), 2 * 256 + 16);
        assert_eq!(FXP_CTRL_STATS_OFF, FXP_NTXCB * 256);
        assert_eq!(FXP_CTRL_U_OFF, FXP_NTXCB * 256 + 68);
        // No transmit block straddles a page: 16 of them fill each 4 KiB page.
        assert_eq!(4096 % size_of::<FxpCbTx>(), 0);
    }
}
/* </TESTS> */
