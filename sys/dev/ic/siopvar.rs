/*	$OpenBSD: siopvar.h,v 1.18 2024/05/13 01:15:50 jsg Exp $ */
/*	$NetBSD: siopvar.h,v 1.22 2005/11/18 23:10:32 bouyer Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2000 Manuel Bouyer.
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
 *
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/ic/siopvar.h>`: structure and definitions for the siop driver: the DMA memory
//! wrapper, the per-command SCRIPTS area (`struct siop_xfer`), the command blocks and their
//! queues, the per-tag, per-lun and per-target state, the lun switches in the script RAM and
//! the adapter's softc.
//!
//! Upstream: sys/dev/ic/siopvar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct siop_xfer` is in DMA memory: `#[repr(C)]`, the tables first and `resel[]` a
//!   `Cell` array, accessed volatile like the tables (`siopvar_common.rs`); its 384 bytes are
//!   asserted.
//! - `SIOP_DMA_MAP`, `SIOP_DMA_DVA` and `SIOP_DMA_KVA` are the methods `map`, `dva` and
//!   `kva` of [`SiopDmamem`].
//! - `#define cmd_tables cmd_c.siop_tables` is [`SiopCmd::tables`].
//! - The queues (`TAILQ_HEAD(cmd_list, siop_cmd)`, `cbd_list`, `lunsw_list`) are
//!   `TailqHead`s of `sys/queue.rs` over the `next` entries.
//! - Pointers to `malloc`ed structures (`siop_cbdp`, `cmds`, `xfers`, `sense`, `siop_lun[]`,
//!   `lunsw`, `active`) are `Option`s of `NonNull` or of `&'static` references (the command
//!   blocks, never freed once made); their accessors panic where the C would dereference
//!   NULL.
//! - `struct siop_softc` reaches the targets of its common softc as [`SiopTarget`]s through
//!   [`SiopSoftc::target`], the cast the C writes `(struct siop_target *)sc->sc_c.targets[i]`.
//! - The prototypes of `siop.c`'s functions are the functions of `siop.rs`.

use core::cell::Cell;
use core::ptr::NonNull;

use crate::dev::ic::siopvar_common::{
    SiopCommonCmd, SiopCommonSoftc, SiopCommonTarget, SiopCommonXfer,
};
use crate::kern::subr_prf::panic;
use crate::machine::bus::{BusAddr, BusDmaSegment, BusDmamap};
use crate::queue_adapter;
use crate::scsi::scsiconf::ScsiIopool;
use crate::sys::queue::{TailqEntry, TailqHead};

/// `SIOP_NTAG`: number of tags.
pub const SIOP_NTAG: usize = 16;

/// `struct siop_dmamem`: wrap up the bus_dma api.
pub struct SiopDmamem {
    /// `sdm_map`.
    pub sdm_map: &'static BusDmamap,
    /// `sdm_seg`.
    pub sdm_seg: BusDmaSegment,
    /// `sdm_size`.
    pub sdm_size: usize,
    /// `sdm_kva`.
    pub sdm_kva: NonNull<u8>,
}

impl SiopDmamem {
    /// `SIOP_DMA_MAP(sdm)`.
    pub fn map(&self) -> &'static BusDmamap {
        self.sdm_map
    }

    /// `SIOP_DMA_DVA(sdm)`: the device address of the area (its map's first segment).
    pub fn dva(&self) -> BusAddr {
        match self.sdm_map.dm_segs().first() {
            Some(seg) => seg.get().ds_addr,
            None => panic(format_args!("siop: DMA memory without a segment")),
        }
    }

    /// `SIOP_DMA_KVA(sdm)`.
    pub fn kva(&self) -> NonNull<u8> {
        self.sdm_kva
    }
}

/// `struct siop_xfer`: xfer description of the script: tables and reselect script. In
/// `struct siop_common_cmd`, `siop_tables` will point to this.
#[repr(C)]
pub struct SiopXfer {
    /// `siop_tables`.
    pub siop_tables: SiopCommonXfer,
    /// `resel[]`: the copy of `load_dsa` (`u_int32_t resel[sizeof(load_dsa) /
    /// sizeof(load_dsa[0])]`), with some entries added to make size 384 bytes (244+140).
    pub resel: [Cell<u32>; 35],
}

/// `struct siop_cmd`: a command handled by the SCSI controller. These are chained in either
/// a free list or an active list. We have one queue per target.
#[repr(C)]
pub struct SiopCmd {
    /// `next`.
    pub next: TailqEntry<SiopCmd>,
    /// `cmd_c`.
    pub cmd_c: SiopCommonCmd,
    /// `siop_cbdp`: pointer to our siop_cbd.
    pub siop_cbdp: Cell<Option<&'static SiopCbd>>,
    /// `reselslot`.
    pub reselslot: Cell<i32>,
    /// `saved_offset`: offset in table after disc without sdp.
    pub saved_offset: Cell<u32>,
}

impl SiopCmd {
    /// `siop_cmd->cmd_tables` (`cmd_c.siop_tables`).
    pub fn tables(&self) -> &'static SiopCommonXfer {
        self.cmd_c.tables()
    }

    /// `(struct siop_xfer *)siop_cmd->cmd_tables`: the tables with the select/reselect
    /// script behind them.
    pub fn xfer(&self) -> &'static SiopXfer {
        // SAFETY: `siop_tables` always points at the `siop_tables` member (the first) of a
        // `SiopXfer` in the command block's DMA page (siop_morecbd), mapped forever.
        unsafe { &*core::ptr::from_ref(self.tables()).cast::<SiopXfer>() }
    }

    /// `(struct siop_target *)siop_cmd->cmd_c.siop_target`.
    pub fn siop_target(&self) -> &'static SiopTarget {
        // SAFETY: siop_scsicmd sets `siop_target` from the adapter's `targets[]`.
        unsafe { SiopTarget::from_common(self.cmd_c.target()) }
    }

    /// `siop_cmd->siop_cbdp`, set when the command block is made.
    pub fn cbd(&self) -> &'static SiopCbd {
        match self.siop_cbdp.get() {
            Some(cbd) => cbd,
            None => panic(format_args!("siop: command without a block")),
        }
    }
}

/// `struct siop_cbd`: command block descriptors: an array of siop_cmd, siop_xfer, and
/// sense.
#[repr(C)]
pub struct SiopCbd {
    /// `next`.
    pub next: TailqEntry<SiopCbd>,
    /// `cmds`: `SIOP_NCMDPB` commands.
    pub cmds: Cell<*mut SiopCmd>,
    /// `xfers`.
    pub xfers: Cell<Option<&'static SiopDmamem>>,
    /// `sense`.
    pub sense: Cell<Option<&'static SiopDmamem>>,
}

impl SiopCbd {
    /// `cbdp->xfers`.
    pub fn xfers(&self) -> &'static SiopDmamem {
        match self.xfers.get() {
            Some(x) => x,
            None => panic(format_args!("siop: command block without tables")),
        }
    }
}

/// `struct siop_tag`: per-tag struct.
#[repr(C)]
pub struct SiopTag {
    /// `active`: active command.
    pub active: Cell<Option<&'static SiopCmd>>,
    /// `reseloff`.
    pub reseloff: Cell<u32>,
}

/// `struct siop_lun`: per lun struct.
#[repr(C)]
pub struct SiopLun {
    /// `siop_tag[]`: tag array.
    pub siop_tag: [SiopTag; SIOP_NTAG],
    /// `lun_flags`: `SIOP_LUNF_*`.
    pub lun_flags: Cell<i32>,
    /// `reseloff`.
    pub reseloff: Cell<u32>,
}

/// `SIOP_LUNF_FULL`: queue full message.
pub const SIOP_LUNF_FULL: i32 = 0x01;

/// `struct siop_target`: per target struct; `siop_common_cmd->target` and
/// `siop_common_softc->targets[]` will point to this.
#[repr(C)]
pub struct SiopTarget {
    /// `target_c`.
    pub target_c: SiopCommonTarget,
    /// `siop_lun[]`: per-lun state.
    pub siop_lun: [Cell<Option<NonNull<SiopLun>>>; 8],
    /// `reseloff`.
    pub reseloff: Cell<u32>,
    /// `lunsw`.
    pub lunsw: Cell<Option<NonNull<SiopLunsw>>>,
}

impl SiopTarget {
    /// `(struct siop_target *)target`: the target whose common part `t` is.
    ///
    /// # Safety
    ///
    /// `t` is the `target_c` of a `SiopTarget`: an entry of a siop adapter's `targets[]`
    /// (siop_scsiprobe allocates them all; esiop is not ported) or a command's
    /// `siop_target`, copied from there.
    pub unsafe fn from_common(t: &SiopCommonTarget) -> &SiopTarget {
        // SAFETY: the caller's guarantee; `target_c` is the first member of the
        // `#[repr(C)]` `SiopTarget`.
        unsafe { &*core::ptr::from_ref(t).cast::<SiopTarget>() }
    }

    /// `siop_target->siop_lun[lun]`, `None` for NULL.
    pub fn lun(&self, lun: usize) -> Option<&'static SiopLun> {
        // SAFETY: a non-NULL entry was allocated by siop_scsiprobe and is freed by
        // siop_scsifree after the entry is cleared, under the kernel lock.
        self.siop_lun[lun].get().map(|l| unsafe { &*l.as_ptr() })
    }

    /// `siop_target->lunsw`, which every target has once siop_scsiprobe made it.
    pub fn lunsw(&self) -> &'static SiopLunsw {
        match self.lunsw.get() {
            // SAFETY: lun switches are freed only by siop_reset, which replaces this
            // pointer at once.
            Some(l) => unsafe { &*l.as_ptr() },
            None => panic(format_args!("siop: target without a lun switch")),
        }
    }
}

/// `struct siop_lunsw`.
#[repr(C)]
pub struct SiopLunsw {
    /// `next`.
    pub next: TailqEntry<SiopLunsw>,
    /// `lunsw_off`: offset of this lun sw, from sc_scriptaddr.
    pub lunsw_off: Cell<u32>,
    /// `lunsw_size`: size of this lun sw.
    pub lunsw_size: Cell<u32>,
}

queue_adapter!(
    /// `TAILQ_HEAD(cmd_list, siop_cmd)`, through `next`.
    pub CmdList: SiopCmd, next => TailqEntry<SiopCmd>
);
queue_adapter!(
    /// `TAILQ_HEAD(cbd_list, siop_cbd)`, through `next`.
    pub CbdList: SiopCbd, next => TailqEntry<SiopCbd>
);
queue_adapter!(
    /// `TAILQ_HEAD(lunsw_list, siop_lunsw)`, through `next`.
    pub LunswList: SiopLunsw, next => TailqEntry<SiopLunsw>
);

/// `struct siop_softc`: driver internal state.
#[repr(C)]
pub struct SiopSoftc {
    /// `sc_c`.
    pub sc_c: SiopCommonSoftc,
    /// `sc_currschedslot`: current scheduler slot.
    pub sc_currschedslot: Cell<i32>,
    /// `cmds`: list of command block descriptors.
    pub cmds: TailqHead<CbdList>,
    /// `free_list`: cmd descr free list.
    pub free_list: TailqHead<CmdList>,
    /// `urgent_list`: high priority cmd descr list.
    pub urgent_list: TailqHead<CmdList>,
    /// `ready_list`: cmd descr ready list.
    pub ready_list: TailqHead<CmdList>,
    /// `iopool`: cmd pool.
    pub iopool: ScsiIopool,
    /// `lunsw_list`: lunsw free list.
    pub lunsw_list: TailqHead<LunswList>,
    /// `script_free_lo`: free ram offset from sc_scriptaddr.
    pub script_free_lo: Cell<u32>,
    /// `script_free_hi`: free ram offset from sc_scriptaddr.
    pub script_free_hi: Cell<u32>,
    /// `sc_ntargets`: number of known targets.
    pub sc_ntargets: Cell<i32>,
    /// `sc_flags`: `SCF_*`.
    pub sc_flags: Cell<u32>,
}

impl SiopSoftc {
    /// `(struct siop_target *)sc->sc_c.targets[target]`, `None` for NULL.
    pub fn target(&self, target: usize) -> Option<&'static SiopTarget> {
        // SAFETY: every entry of a siop adapter's `targets[]` is a `SiopTarget` that
        // siop_scsiprobe allocated.
        self.sc_c
            .target_opt(target)
            .map(|t| unsafe { SiopTarget::from_common(t) })
    }
}

/// `SCF_CHAN_NOSLOT`: channel out of scheduler slot.
pub const SCF_CHAN_NOSLOT: u32 = 0x0001;

const _: () = {
    assert!(size_of::<SiopXfer>() == 384);
    assert!(core::mem::offset_of!(SiopXfer, siop_tables) == 0);
    assert!(core::mem::offset_of!(SiopTarget, target_c) == 0);
};
/* </CODE> */
