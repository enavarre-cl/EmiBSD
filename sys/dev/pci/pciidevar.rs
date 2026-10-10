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
 * Copyright (c) 1998 Christopher G. Demetriou.  All rights reserved.
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
 *      This product includes software developed by Christopher G. Demetriou
 *	for the NetBSD Project.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission
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

/* <CODE> */
//! PCI IDE controller driver definitions: the controller softc that `pciide.c` and its
//! chip-specific code share, with its per-channel state and DMA maps.
//!
//! Upstream: sys/dev/pci/pciidevar.h @ 3ce1f3f79392
//!
//! Author: Christopher G. Demetriou, March 2, 1998.
//!
//! ## Deviations
//! - `struct pciide_softc` and `struct pciide_channel` are `#[repr(C)]` with the generic
//!   parts (`WdcSoftc`, `ChannelSoftc`) first, as `wdc.c` hands back pointers to them; the
//!   members the driver changes after attach are `Cell`s (shared by reference between the
//!   attach path, the interrupt handlers and `wdc.c`'s hooks under `splbio`, as the C
//!   shares them by pointer). Tags and handles are `Cell<Option<..>>`s with accessors that
//!   panic where the C would use an unset one.
//! - The `PCIIDE_DMA*_READ/WRITE` macros are methods of [`PciideSoftc`]; the hooks they call
//!   through are `fn(&PciideSoftc, i32, ..)` pointers.
//! - `sc_pp` is an `Option` (zero until `pciide_attach` sets it); `name` is the channel's
//!   `&'static str`, `None` until `pciide_chansetup`.
//! - `struct pciide_dma_maps`'s maps are `&'static` bus DMA maps (`bus_dmamap_t`), its table
//!   a raw pointer to the `bus_dmamem_map`ped descriptors.
//! - The function prototypes are `pciide.rs`'s; `pciide_machdep_compat_intr_establish` and
//!   `pciide_machdep_compat_intr_disestablish` are the machine's
//!   (`machine::pciide_machdep`), and `gcsc_chip_map` (`__i386__` only) has no counterpart.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;

use crate::dev::ic::wdcvar::{ChannelSoftc, WdcChannelPtr, WdcSoftc};
use crate::dev::pci::pciide::PciideProductDesc;
use crate::dev::pci::pciidereg::IdedmaTable;
use crate::dev::pci::pcivar::Pcireg;
use crate::kern::subr_prf::panic;
use crate::machine::bus::{BusDmaTag, BusDmamap, BusSize, BusSpaceHandle, BusSpaceTag};
use crate::machine::pci_machdep::{PciChipsetTag, Pcitag};
use crate::sys::device::Softc;

/// `PCIIDE_MAX_CHANNELS`.
pub const PCIIDE_MAX_CHANNELS: usize = 4;

/// `sc_dmacmd_read`, `sc_dmactl_read`: an 8-bit DMA register of a channel.
pub type PciideDmaRead = fn(&PciideSoftc, i32) -> u8;
/// `sc_dmacmd_write`, `sc_dmactl_write`.
pub type PciideDmaWrite = fn(&PciideSoftc, i32, u8);
/// `sc_dmatbl_write`.
pub type PciideDmaTblWrite = fn(&PciideSoftc, i32, u32);
/// `chip_unmap`.
pub type PciideChipUnmap = fn(&PciideSoftc, i32);

/// `struct pciide_dma_maps`: DMA tables and DMA map for xfer, for each drive.
pub struct PciideDmaMaps {
    /// `dmamap_table`.
    pub dmamap_table: Cell<Option<&'static BusDmamap>>,
    /// `dma_table`: the descriptors, `NIDEDMA_TABLES` of them.
    pub dma_table: Cell<*mut IdedmaTable>,
    /// `dmamap_xfer`.
    pub dmamap_xfer: Cell<Option<&'static BusDmamap>>,
    /// `dma_flags`.
    pub dma_flags: Cell<i32>,
}

impl PciideDmaMaps {
    /// All zero.
    pub const fn new() -> Self {
        Self {
            dmamap_table: Cell::new(None),
            dma_table: Cell::new(core::ptr::null_mut()),
            dmamap_xfer: Cell::new(None),
            dma_flags: Cell::new(0),
        }
    }
}

impl Default for PciideDmaMaps {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct pciide_channel`: per-channel data.
#[repr(C)]
pub struct PciideChannel {
    /// `wdc_channel`: generic part.
    pub wdc_channel: ChannelSoftc,
    /// `name`.
    pub name: Cell<Option<&'static str>>,
    /// `hw_ok`: hardware mapped & OK?
    pub hw_ok: Cell<i32>,
    /// `compat`: is it compat?
    pub compat: Cell<i32>,
    /// `dma_in_progress`.
    pub dma_in_progress: Cell<i32>,
    /// `ih`: compat or pci handle.
    pub ih: Cell<*mut c_void>,
    /// `ctl_baseioh`: ctrl regs blk, native mode.
    pub ctl_baseioh: Cell<Option<BusSpaceHandle>>,
    /// `dma_maps`: DMA tables and DMA map for xfer, for each drive.
    pub dma_maps: [PciideDmaMaps; 2],
    /// `idedma_cmd`: some controllers require certain bits to always be set for proper
    /// operation of the controller. Set those bits here, if they're required.
    pub idedma_cmd: Cell<u8>,
}

impl PciideChannel {
    /// All zero, as in an `M_ZERO` softc.
    pub const fn new() -> Self {
        Self {
            wdc_channel: ChannelSoftc::new(),
            name: Cell::new(None),
            hw_ok: Cell::new(0),
            compat: Cell::new(0),
            dma_in_progress: Cell::new(0),
            ih: Cell::new(core::ptr::null_mut()),
            ctl_baseioh: Cell::new(None),
            dma_maps: [PciideDmaMaps::new(), PciideDmaMaps::new()],
            idedma_cmd: Cell::new(0),
        }
    }

    /// `cp->name` for `%s`.
    pub fn name(&self) -> &'static str {
        self.name.get().unwrap_or("")
    }

    /// `&cp->dma_maps[drive]`.
    pub fn dma_maps(&self, drive: i32) -> &PciideDmaMaps {
        &self.dma_maps[drive as usize]
    }
}

impl Default for PciideChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct pciide_softc`.
#[repr(C)]
pub struct PciideSoftc {
    /// `sc_wdcdev`: common wdc definitions.
    pub sc_wdcdev: WdcSoftc,
    /// `sc_pc`: PCI registers info.
    pub sc_pc: Cell<Option<PciChipsetTag>>,
    /// `sc_tag`.
    pub sc_tag: Cell<Pcitag>,
    /// `sc_pci_ih`: PCI interrupt handle.
    pub sc_pci_ih: Cell<*mut c_void>,
    /// `sc_dma_ok`: bus-master DMA info.
    pub sc_dma_ok: Cell<i32>,
    /// `sc_dma_iot`.
    pub sc_dma_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_dma_ioh`.
    pub sc_dma_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_dma_iosz`.
    pub sc_dma_iosz: Cell<BusSize>,
    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,

    // Some controllers might have DMA restrictions other than the norm.
    /// `sc_dma_maxsegsz`.
    pub sc_dma_maxsegsz: Cell<BusSize>,
    /// `sc_dma_boundary`.
    pub sc_dma_boundary: Cell<BusSize>,

    /// `sc_save`: used as a register save space by `pciide_activate()`, for the 6 pci regs
    /// starting at `PCI_MAPREG_END + 0x18`; most IDE chipsets need a subset of those saved.
    pub sc_save: [Cell<Pcireg>; 6],
    /// `sc_save2`: up to 6 other registers, which specific chips might need saved.
    pub sc_save2: [Cell<Pcireg>; 6],

    /// `sc_pp`: chip description.
    pub sc_pp: Cell<Option<&'static PciideProductDesc>>,
    /// `chip_unmap`: unmap/detach.
    pub chip_unmap: Cell<Option<PciideChipUnmap>>,
    /// `sc_rev`: chip revision.
    pub sc_rev: Cell<i32>,
    /// `wdc_chanarray`: common definitions.
    pub wdc_chanarray: [WdcChannelPtr; PCIIDE_MAX_CHANNELS],
    /// `pciide_channels`: internal bookkeeping, per-channel data.
    pub pciide_channels: [PciideChannel; PCIIDE_MAX_CHANNELS],

    /// `sc_cookie`: chip-specific private data.
    pub sc_cookie: Cell<*mut c_void>,
    /// `sc_cookielen`.
    pub sc_cookielen: Cell<usize>,

    // DMA registers access functions
    /// `sc_dmacmd_read`.
    pub sc_dmacmd_read: Cell<Option<PciideDmaRead>>,
    /// `sc_dmacmd_write`.
    pub sc_dmacmd_write: Cell<Option<PciideDmaWrite>>,
    /// `sc_dmactl_read`.
    pub sc_dmactl_read: Cell<Option<PciideDmaRead>>,
    /// `sc_dmactl_write`.
    pub sc_dmactl_write: Cell<Option<PciideDmaWrite>>,
    /// `sc_dmatbl_write`.
    pub sc_dmatbl_write: Cell<Option<PciideDmaTblWrite>>,
}

impl PciideSoftc {
    /// `sc->sc_wdcdev.sc_dev.dv_xname`.
    pub fn xname(&self) -> &str {
        self.sc_wdcdev.sc_dev.xname()
    }

    /// `sc->sc_pc`, set by `pciide_attach`.
    pub fn pc(&self) -> PciChipsetTag {
        match self.sc_pc.get() {
            Some(pc) => pc,
            None => panic(format_args!("{}: sc_pc not set", self.xname())),
        }
    }

    /// `sc->sc_tag`.
    pub fn tag(&self) -> Pcitag {
        self.sc_tag.get()
    }

    /// `sc->sc_pp`, set by `pciide_attach`.
    pub fn pp(&self) -> &'static PciideProductDesc {
        match self.sc_pp.get() {
            Some(pp) => pp,
            None => panic(format_args!("{}: sc_pp not set", self.xname())),
        }
    }

    /// `sc->sc_dma_iot`, set by `pciide_mapreg_dma` (or a chip's own mapping).
    pub fn dma_iot(&self) -> BusSpaceTag {
        match self.sc_dma_iot.get() {
            Some(t) => t,
            None => panic(format_args!("{}: sc_dma_iot not set", self.xname())),
        }
    }

    /// `sc->sc_dma_ioh`.
    pub fn dma_ioh(&self) -> BusSpaceHandle {
        match self.sc_dma_ioh.get() {
            Some(h) => h,
            None => panic(format_args!("{}: sc_dma_ioh not set", self.xname())),
        }
    }

    /// `sc->sc_dmat`.
    pub fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => panic(format_args!("{}: sc_dmat not set", self.xname())),
        }
    }

    /// `&sc->pciide_channels[chan]`.
    pub fn channel(&self, chan: i32) -> &PciideChannel {
        &self.pciide_channels[chan as usize]
    }

    /// `PCIIDE_DMACMD_READ(sc, chan)`.
    pub fn dmacmd_read(&self, chan: i32) -> u8 {
        match self.sc_dmacmd_read.get() {
            Some(f) => f(self, chan),
            None => panic(format_args!("{}: no sc_dmacmd_read", self.xname())),
        }
    }

    /// `PCIIDE_DMACMD_WRITE(sc, chan, val)`.
    pub fn dmacmd_write(&self, chan: i32, val: u8) {
        match self.sc_dmacmd_write.get() {
            Some(f) => f(self, chan, val),
            None => panic(format_args!("{}: no sc_dmacmd_write", self.xname())),
        }
    }

    /// `PCIIDE_DMACTL_READ(sc, chan)`.
    pub fn dmactl_read(&self, chan: i32) -> u8 {
        match self.sc_dmactl_read.get() {
            Some(f) => f(self, chan),
            None => panic(format_args!("{}: no sc_dmactl_read", self.xname())),
        }
    }

    /// `PCIIDE_DMACTL_WRITE(sc, chan, val)`.
    pub fn dmactl_write(&self, chan: i32, val: u8) {
        match self.sc_dmactl_write.get() {
            Some(f) => f(self, chan, val),
            None => panic(format_args!("{}: no sc_dmactl_write", self.xname())),
        }
    }

    /// `PCIIDE_DMATBL_WRITE(sc, chan, val)`.
    pub fn dmatbl_write(&self, chan: i32, val: u32) {
        match self.sc_dmatbl_write.get() {
            Some(f) => f(self, chan, val),
            None => panic(format_args!("{}: no sc_dmatbl_write", self.xname())),
        }
    }

    /// `sc->sc_cookie` as the chip's private structure.
    ///
    /// # Safety
    ///
    /// `sc_cookie` was set by this chip's `chip_map` to a live allocation of a `T`.
    pub unsafe fn cookie<T>(&self) -> &T {
        let Some(p) = NonNull::new(self.sc_cookie.get().cast::<T>()) else {
            panic(format_args!("{}: no sc_cookie", self.xname()));
        };
        // SAFETY: the caller's guarantee; the cookie lives as long as the softc.
        unsafe { p.as_ref() }
    }
}

// SAFETY: `#[repr(C)]` with the wdc softc (itself headed by the device, all-zero valid)
// first; the other members are `Cell`s of integers, raw pointers and `Option`s of tags,
// handles, references and function pointers, and channels made of the same, all valid as
// all zero bits.
unsafe impl Softc for PciideSoftc {}
/* </CODE> */
