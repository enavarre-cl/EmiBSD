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

/*-
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum, by Onno van der Linden and by Manuel Bouyer.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *	notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *	notice, this list of conditions and the following disclaimer in the
 *	documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE NETBSD FOUNDATION, INC. AND CONTRIBUTORS
 * ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
 * TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION OR CONTRIBUTORS
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
//! The IDE channel and controller state shared by `wdc.c`, the disk and ATAPI drivers and
//! the bus front-ends (`pciide`): the per-channel softc, its register access table, the
//! controller softc with its DMA and mode hooks, and the transfer (`struct wdc_xfer`) that
//! the channel's queue runs.
//!
//! Upstream: sys/dev/ic/wdcvar.h @ 3ce1f3f79392
//!
//! A front-end embeds a [`WdcSoftc`] at the head of its softc and a [`ChannelSoftc`] at the
//! head of each of its channel structures (`struct pciide_channel`), so both are
//! `#[repr(C)]` and the front-end's hooks cast them back, as the C does.
//!
//! ## Deviations
//! - The members changed after attach are `Cell`s: the softcs and transfers are shared by
//!   reference between the attach path, the interrupt handler, the timeout and the drive
//!   drivers, under `splbio`, as the C shares them by pointer. The bus tags and handles are
//!   `Cell<Option<..>>`s, set by the front-end before `wdcattach`, read through
//!   [`ChannelSoftc::cmd_iot`] and its siblings, which panic where the C would use an
//!   unmapped handle.
//! - `enum wdc_regs` has members with equal values (`wdr_seccnt` and `wdr_ireason`), which a
//!   Rust `enum` cannot have: it is the newtype [`WdcRegs`] with the C names as constants.
//! - `struct channel_softc_vtbl`'s raw transfers take the data as an optional slice: `None`
//!   is the C's NULL, which reads into (or writes zeros from) the bit bucket `nbytes` bytes
//!   long. The `CHP_*` macros are methods of [`ChannelSoftc`]; `_vtbl` must be set (as
//!   `wdcprobe` and `wdcattach` do) before they are used.
//! - The `wdc_softc` hooks taking the `void *dma_arg` cookie are `unsafe fn`s; `dma_init`
//!   returns `Result` (`EINVAL` still means "no DMA for this transfer"). `channels` is a
//!   slice of the front-end's array of channel pointers.
//! - `struct wdc_xfer`'s `cmd` stays a `void *` (`wdc_command`, `ata_bio` or an ATAPI
//!   request); its `chp` is a pointer to the channel. `struct atapi_return_args` belongs to
//!   `atapiscsi.c`, which is not ported; it is the opaque [`AtapiReturnArgs`].
//! - The `wdcwait`, `wait_for_drq`, `wait_for_unbusy` and `wait_for_ready` macros are
//!   functions returning `Err(ETIMEDOUT)` for the C's -1. The prototypes are `wdc.rs`'s.

#![allow(non_upper_case_globals)] // the `wdr_*` register names

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;

use crate::dev::ata::atavar::AtaDriveDatas;
use crate::kern::subr_prf::panic;
use crate::machine::bus::{BusSize, BusSpaceHandle, BusSpaceTag};
use crate::queue_adapter;
use crate::sys::device::{Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::queue::{ListEntry, TailqEntry, TailqHead};
use crate::sys::timeout::Timeout;

/// `WDC_OPTION_PROBE_VERBOSE`: a `cf_flags` bit asking for a verbose probe.
pub const WDC_OPTION_PROBE_VERBOSE: i32 = 0x10000;

/// `WDCF_ACTIVE`: channel is active.
pub const WDCF_ACTIVE: i32 = 0x01;
/// `WDCF_ONESLAVE`: slave-only channel.
pub const WDCF_ONESLAVE: i32 = 0x02;
/// `WDCF_IRQ_WAIT`: controller is waiting for irq.
pub const WDCF_IRQ_WAIT: i32 = 0x10;
/// `WDCF_DMA_WAIT`: controller is waiting for DMA.
pub const WDCF_DMA_WAIT: i32 = 0x20;
/// `WDCF_VERBOSE_PROBE`: verbose probe.
pub const WDCF_VERBOSE_PROBE: i32 = 0x40;
/// `WDCF_DMA_BEFORE_CMD`: start dma before a command.
pub const WDCF_DMA_BEFORE_CMD: i32 = 0x80;

/*
 * Disk Controller register definitions.
 */

/// `_WDC_REGMASK`.
pub const _WDC_REGMASK: i32 = 7;
/// `_WDC_AUX`.
pub const _WDC_AUX: i32 = 8;
/// `_WDC_RDONLY`.
pub const _WDC_RDONLY: i32 = 16;
/// `_WDC_WRONLY`.
pub const _WDC_WRONLY: i32 = 32;

/// `WDC_NREG`: number of command registers.
pub const WDC_NREG: i32 = 8;
/// `WDC_NSHADOWREG`: number of command "shadow" registers.
pub const WDC_NSHADOWREG: i32 = 2;

/* Capabilities supported by the controller */

/// `WDC_CAPABILITY_DATA16`: can do 16-bit data access.
pub const WDC_CAPABILITY_DATA16: i32 = 0x0001;
/// `WDC_CAPABILITY_DATA32`: can do 32-bit data access.
pub const WDC_CAPABILITY_DATA32: i32 = 0x0002;
/// `WDC_CAPABILITY_MODE`: controller knows its PIO/DMA modes.
pub const WDC_CAPABILITY_MODE: i32 = 0x0004;
/// `WDC_CAPABILITY_DMA`: DMA.
pub const WDC_CAPABILITY_DMA: i32 = 0x0008;
/// `WDC_CAPABILITY_UDMA`: Ultra-DMA/33.
pub const WDC_CAPABILITY_UDMA: i32 = 0x0010;
/// `WDC_CAPABILITY_NO_EXTRA_RESETS`: only reset once.
pub const WDC_CAPABILITY_NO_EXTRA_RESETS: i32 = 0x0100;
/// `WDC_CAPABILITY_PREATA`: ctrl can be a pre-ata one.
pub const WDC_CAPABILITY_PREATA: i32 = 0x0200;
/// `WDC_CAPABILITY_IRQACK`: callback to ack interrupt.
pub const WDC_CAPABILITY_IRQACK: i32 = 0x0400;
/// `WDC_CAPABILITY_SINGLE_DRIVE`: don't probe second drive.
pub const WDC_CAPABILITY_SINGLE_DRIVE: i32 = 0x800;
/// `WDC_CAPABILITY_NO_ATAPI_DMA`: don't do DMA with ATAPI.
pub const WDC_CAPABILITY_NO_ATAPI_DMA: i32 = 0x1000;
/// `WDC_CAPABILITY_SATA`: SATA controller.
pub const WDC_CAPABILITY_SATA: i32 = 0x2000;

/// `WDC_QUIRK_NOSHORTDMA`: can't do short DMA transfers.
pub const WDC_QUIRK_NOSHORTDMA: u16 = 0x0001;
/// `WDC_QUIRK_NOATA`: skip attaching ATA disks.
pub const WDC_QUIRK_NOATA: u16 = 0x0002;
/// `WDC_QUIRK_NOATAPI`: skip attaching ATAPI devices.
pub const WDC_QUIRK_NOATAPI: u16 = 0x0004;

/* flags passed to DMA functions */

/// `WDC_DMA_READ`.
pub const WDC_DMA_READ: i32 = 0x01;
/// `WDC_DMA_IRQW`.
pub const WDC_DMA_IRQW: i32 = 0x02;
/// `WDC_DMA_LBA48`.
pub const WDC_DMA_LBA48: i32 = 0x04;

/// `WDC_DMAST_NOIRQ`: missing IRQ.
pub const WDC_DMAST_NOIRQ: i32 = 0x01;
/// `WDC_DMAST_ERR`: DMA error.
pub const WDC_DMAST_ERR: i32 = 0x02;
/// `WDC_DMAST_UNDER`: DMA underrun.
pub const WDC_DMAST_UNDER: i32 = 0x04;

/// `C_ATAPI`: xfer is ATAPI request.
pub const C_ATAPI: u32 = 0x0002;
/// `C_TIMEOU`: xfer processing timed out.
pub const C_TIMEOU: u32 = 0x0004;
/// `C_NEEDDONE`: need to call upper-level done.
pub const C_NEEDDONE: u32 = 0x0010;
/// `C_POLL`: cmd is polled.
pub const C_POLL: u32 = 0x0020;
/// `C_DMA`: cmd uses DMA.
pub const C_DMA: u32 = 0x0040;
/// `C_SENSE`: cmd is a internal command.
pub const C_SENSE: u32 = 0x0080;
/// `C_MEDIA_ACCESS`: is a media access command.
pub const C_MEDIA_ACCESS: u32 = 0x0100;
/// `C_POLL_MACHINE`: machine has a poll handler.
pub const C_POLL_MACHINE: u32 = 0x0200;
/// `C_PRIVATEXFER`: privately managed xfer.
pub const C_PRIVATEXFER: u32 = 0x0400;
/// `C_SCSIXFER`: SCSI managed xfer.
pub const C_SCSIXFER: u32 = 0x0800;

/// `WDC_CANSLEEP`: `wdc_get_xfer` may sleep.
pub const WDC_CANSLEEP: i32 = 0x00;
/// `WDC_NOSLEEP`: `wdc_get_xfer` may not sleep.
pub const WDC_NOSLEEP: i32 = 0x01;

/// `NOWAIT`: `wdcreset` does not wait for the drives.
pub const NOWAIT: i32 = 0x02;
/// `VERBOSE`: `wdcreset` prints the drives that failed.
pub const VERBOSE: i32 = 0x01;
/// `SILENT`: `wdcreset` will not print errors.
pub const SILENT: i32 = 0x00;

/// `WDC_RESET_WAIT`: ATA/ATAPI specs says a device can take 31s to reset.
pub const WDC_RESET_WAIT: i32 = 31000;

/// `enum wdc_regs`: a task file register, its offset in the low bits with `_WDC_AUX` for the
/// control block and `_WDC_RDONLY`/`_WDC_WRONLY` for the direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WdcRegs(pub i32);

/// `wdr_error`.
pub const wdr_error: WdcRegs = WdcRegs(_WDC_RDONLY | 1);
/// `wdr_features`.
pub const wdr_features: WdcRegs = WdcRegs(_WDC_WRONLY | 1);
/// `wdr_seccnt`.
pub const wdr_seccnt: WdcRegs = WdcRegs(2);
/// `wdr_ireason`.
pub const wdr_ireason: WdcRegs = WdcRegs(2);
/// `wdr_sector`.
pub const wdr_sector: WdcRegs = WdcRegs(3);
/// `wdr_lba_lo`.
pub const wdr_lba_lo: WdcRegs = WdcRegs(3);
/// `wdr_cyl_lo`.
pub const wdr_cyl_lo: WdcRegs = WdcRegs(4);
/// `wdr_lba_mi`.
pub const wdr_lba_mi: WdcRegs = WdcRegs(4);
/// `wdr_cyl_hi`.
pub const wdr_cyl_hi: WdcRegs = WdcRegs(5);
/// `wdr_lba_hi`.
pub const wdr_lba_hi: WdcRegs = WdcRegs(5);
/// `wdr_sdh`.
pub const wdr_sdh: WdcRegs = WdcRegs(6);
/// `wdr_status`.
pub const wdr_status: WdcRegs = WdcRegs(_WDC_RDONLY | 7);
/// `wdr_command`.
pub const wdr_command: WdcRegs = WdcRegs(_WDC_WRONLY | 7);
/// `wdr_altsts`.
pub const wdr_altsts: WdcRegs = WdcRegs(_WDC_RDONLY | _WDC_AUX);
/// `wdr_ctlr`.
pub const wdr_ctlr: WdcRegs = WdcRegs(_WDC_WRONLY | _WDC_AUX);

impl WdcRegs {
    /// `reg & _WDC_REGMASK`: the register's offset in its block.
    pub const fn offset(self) -> BusSize {
        (self.0 & _WDC_REGMASK) as BusSize
    }

    /// `reg & f`.
    pub const fn isset(self, f: i32) -> bool {
        self.0 & f != 0
    }
}

queue_adapter!(
    /// `TAILQ_ENTRY(wdc_xfer) c_xferchain`: a channel queue's transfers.
    pub WdcXferChain: WdcXfer, c_xferchain => TailqEntry<WdcXfer>
);

/// `struct channel_queue`: per channel queue (may be shared).
pub struct ChannelQueue {
    /// `sc_xfer`: the transfers, the running one first.
    pub sc_xfer: TailqHead<WdcXferChain>,
}

impl ChannelQueue {
    /// An empty queue, to be `TAILQ_INIT`ed where it lives (`wdc_alloc_queue`).
    pub const fn new() -> Self {
        Self {
            sc_xfer: TailqHead::new(),
        }
    }
}

impl Default for ChannelQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct channel_softc`: per channel data.
#[repr(C)]
pub struct ChannelSoftc {
    /// `_vtbl`: the register access functions.
    pub _vtbl: Cell<Option<&'static ChannelSoftcVtbl>>,

    /* Our location */
    /// `channel`.
    pub channel: Cell<i32>,
    /// `wdc`: our controller's softc.
    pub wdc: Cell<Option<NonNull<WdcSoftc>>>,
    /* Our registers */
    /// `cmd_iot`.
    pub cmd_iot: Cell<Option<BusSpaceTag>>,
    /// `cmd_ioh`.
    pub cmd_ioh: Cell<Option<BusSpaceHandle>>,
    /// `cmd_iosz`.
    pub cmd_iosz: Cell<BusSize>,
    /// `ctl_iot`.
    pub ctl_iot: Cell<Option<BusSpaceTag>>,
    /// `ctl_ioh`.
    pub ctl_ioh: Cell<Option<BusSpaceHandle>>,
    /// `ctl_iosz`.
    pub ctl_iosz: Cell<BusSize>,
    /// `data32iot`: only used for 32 bit xfers.
    pub data32iot: Cell<Option<BusSpaceTag>>,
    /// `data32ioh`: only used for 32 bit xfers.
    pub data32ioh: Cell<Option<BusSpaceHandle>>,
    /* Our state */
    /// `ch_flags` (`WDCF_*`).
    pub ch_flags: Cell<i32>,
    /// `ch_status`: copy of status register.
    pub ch_status: Cell<u8>,
    /// `ch_prev_log_status`: previous logged value of status reg.
    pub ch_prev_log_status: Cell<u8>,
    /// `ch_log_idx`.
    pub ch_log_idx: Cell<u8>,
    /// `ch_error`: copy of error register.
    pub ch_error: Cell<u8>,
    /// `ch_drive`: per-drive infos.
    pub ch_drive: [AtaDriveDatas; 2],

    /// `ch_queue`: channel queues. May be the same for all channels, if hw channels are not
    /// independent.
    pub ch_queue: Cell<Option<NonNull<ChannelQueue>>>,
    /// `ch_timo`.
    pub ch_timo: Timeout,

    /// `dying`.
    pub dying: Cell<i32>,
}

impl ChannelSoftc {
    /// All zero, as in an `M_ZERO` softc.
    pub const fn new() -> Self {
        Self {
            _vtbl: Cell::new(None),
            channel: Cell::new(0),
            wdc: Cell::new(None),
            cmd_iot: Cell::new(None),
            cmd_ioh: Cell::new(None),
            cmd_iosz: Cell::new(0),
            ctl_iot: Cell::new(None),
            ctl_ioh: Cell::new(None),
            ctl_iosz: Cell::new(0),
            data32iot: Cell::new(None),
            data32ioh: Cell::new(None),
            ch_flags: Cell::new(0),
            ch_status: Cell::new(0),
            ch_prev_log_status: Cell::new(0),
            ch_log_idx: Cell::new(0),
            ch_error: Cell::new(0),
            ch_drive: [AtaDriveDatas::new(), AtaDriveDatas::new()],
            ch_queue: Cell::new(None),
            ch_timo: Timeout::zeroed(),
            dying: Cell::new(0),
        }
    }

    /// `chp->wdc`, which the front-end sets before probing the channel; `None` for a channel
    /// probed on its own (`wdcprobe` from an ISA match).
    pub fn wdc_opt(&self) -> Option<&WdcSoftc> {
        // SAFETY: `wdc` is NULL or the controller softc that holds this channel, which
        // outlives it.
        self.wdc.get().map(|p| unsafe { p.as_ref() })
    }

    /// `chp->wdc`. Panics where the C would dereference NULL.
    pub fn wdc(&self) -> &WdcSoftc {
        match self.wdc_opt() {
            Some(wdc) => wdc,
            None => panic(format_args!(
                "wdc: channel {} has no controller",
                self.channel.get()
            )),
        }
    }

    /// `chp->ch_queue`. Panics where the C would dereference NULL.
    pub fn queue(&self) -> &ChannelQueue {
        match self.ch_queue.get() {
            // SAFETY: `ch_queue` is the queue `wdc_alloc_queue` made for the controller,
            // freed only after its channels are detached.
            Some(q) => unsafe { q.as_ref() },
            None => panic(format_args!(
                "wdc: channel {} has no queue",
                self.channel.get()
            )),
        }
    }

    /// `chp->cmd_iot`, set by the front-end.
    pub fn cmd_iot(&self) -> BusSpaceTag {
        match self.cmd_iot.get() {
            Some(t) => t,
            None => panic(format_args!(
                "wdc: channel {}: cmd_iot not set",
                self.channel.get()
            )),
        }
    }

    /// `chp->cmd_ioh`, set by the front-end.
    pub fn cmd_ioh(&self) -> BusSpaceHandle {
        match self.cmd_ioh.get() {
            Some(h) => h,
            None => panic(format_args!(
                "wdc: channel {}: cmd_ioh not set",
                self.channel.get()
            )),
        }
    }

    /// `chp->ctl_iot`, set by the front-end.
    pub fn ctl_iot(&self) -> BusSpaceTag {
        match self.ctl_iot.get() {
            Some(t) => t,
            None => panic(format_args!(
                "wdc: channel {}: ctl_iot not set",
                self.channel.get()
            )),
        }
    }

    /// `chp->ctl_ioh`, set by the front-end.
    pub fn ctl_ioh(&self) -> BusSpaceHandle {
        match self.ctl_ioh.get() {
            Some(h) => h,
            None => panic(format_args!(
                "wdc: channel {}: ctl_ioh not set",
                self.channel.get()
            )),
        }
    }

    /// `chp->ch_flags & f`.
    pub fn isset(&self, f: i32) -> bool {
        self.ch_flags.get() & f != 0
    }

    /// `chp->ch_flags |= f`.
    pub fn set(&self, f: i32) {
        self.ch_flags.set(self.ch_flags.get() | f);
    }

    /// `chp->ch_flags &= ~f`.
    pub fn clr(&self, f: i32) {
        self.ch_flags.set(self.ch_flags.get() & !f);
    }

    /// `chp->_vtbl`. Panics where the C would call through NULL.
    fn vtbl(&self) -> &'static ChannelSoftcVtbl {
        match self._vtbl.get() {
            Some(v) => v,
            None => panic(format_args!(
                "wdc: channel {} has no vtbl",
                self.channel.get()
            )),
        }
    }

    /// `CHP_READ_REG(chp, a)`.
    pub fn read_reg(&self, a: WdcRegs) -> u8 {
        (self.vtbl().read_reg)(self, a)
    }

    /// `CHP_WRITE_REG(chp, a, b)`.
    pub fn write_reg(&self, a: WdcRegs, b: u8) {
        (self.vtbl().write_reg)(self, a, b)
    }

    /// `CHP_LBA48_WRITE_REG(chp, a, b)`.
    pub fn lba48_write_reg(&self, a: WdcRegs, b: u16) {
        (self.vtbl().lba48_write_reg)(self, a, b)
    }

    /// `CHP_READ_RAW_MULTI_2(chp, a, b)`.
    pub fn read_raw_multi_2(&self, a: Option<&mut [u8]>, b: u32) {
        (self.vtbl().read_raw_multi_2)(self, a, b)
    }

    /// `CHP_WRITE_RAW_MULTI_2(chp, a, b)`.
    pub fn write_raw_multi_2(&self, a: Option<&[u8]>, b: u32) {
        (self.vtbl().write_raw_multi_2)(self, a, b)
    }

    /// `CHP_READ_RAW_MULTI_4(chp, a, b)`.
    pub fn read_raw_multi_4(&self, a: Option<&mut [u8]>, b: u32) {
        (self.vtbl().read_raw_multi_4)(self, a, b)
    }

    /// `CHP_WRITE_RAW_MULTI_4(chp, a, b)`.
    pub fn write_raw_multi_4(&self, a: Option<&[u8]>, b: u32) {
        (self.vtbl().write_raw_multi_4)(self, a, b)
    }
}

impl Default for ChannelSoftc {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct channel_softc_vtbl`: how a channel reaches its registers. `wdc_default_vtbl`
/// (`wdc.rs`) is the plain task file; chips with other windows provide their own.
pub struct ChannelSoftcVtbl {
    /// `read_reg`.
    pub read_reg: fn(&ChannelSoftc, WdcRegs) -> u8,
    /// `write_reg`.
    pub write_reg: fn(&ChannelSoftc, WdcRegs, u8),
    /// `lba48_write_reg`.
    pub lba48_write_reg: fn(&ChannelSoftc, WdcRegs, u16),

    /// `read_raw_multi_2`: `nbytes` bytes from the data register into `data`, or discarded
    /// when `data` is `None`.
    pub read_raw_multi_2: fn(&ChannelSoftc, data: Option<&mut [u8]>, nbytes: u32),
    /// `write_raw_multi_2`: `nbytes` bytes of `data` to the data register, or zeros when
    /// `data` is `None`.
    pub write_raw_multi_2: fn(&ChannelSoftc, data: Option<&[u8]>, nbytes: u32),

    /// `read_raw_multi_4`.
    pub read_raw_multi_4: fn(&ChannelSoftc, data: Option<&mut [u8]>, nbytes: u32),
    /// `write_raw_multi_4`.
    pub write_raw_multi_4: fn(&ChannelSoftc, data: Option<&[u8]>, nbytes: u32),
}

/// `int (*dma_init)(void *, int, int, void *, size_t, int)`: sets up the DMA of `datalen`
/// bytes at `databuf` for `drive` of `channel`; `Err(EINVAL)` means this transfer cannot be
/// done by DMA (the caller falls back to PIO).
///
/// # Safety
///
/// `v` is the controller's `dma_arg`, and `databuf` points to `datalen` bytes that stay
/// valid until `dma_finish` returns.
pub type WdcDmaInitFn = unsafe fn(
    v: *mut c_void,
    channel: i32,
    drive: i32,
    databuf: *mut u8,
    datalen: usize,
    flags: i32,
) -> Result<(), Errno>;

/// `void (*dma_start)(void *, int, int)`.
///
/// # Safety
///
/// `v` is the controller's `dma_arg`.
pub type WdcDmaStartFn = unsafe fn(v: *mut c_void, channel: i32, drive: i32);

/// `int (*dma_finish)(void *, int, int, int)`: the `WDC_DMAST_*` status of the transfer
/// (0xff for a controller that went away); `force` halts it.
///
/// # Safety
///
/// `v` is the controller's `dma_arg`.
pub type WdcDmaFinishFn = unsafe fn(v: *mut c_void, channel: i32, drive: i32, force: i32) -> i32;

/// One entry of a front-end's array of channel pointers (`struct channel_softc *`).
pub type WdcChannelPtr = Cell<Option<NonNull<ChannelSoftc>>>;

/// `struct wdc_softc`: per controller state.
#[repr(C)]
#[allow(non_snake_case)] // PIO_cap, DMA_cap, UDMA_cap: the C names
pub struct WdcSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /* mandatory fields */
    /// `cap`: capabilities supported by the controller (`WDC_CAPABILITY_*`).
    pub cap: Cell<i32>,
    /// `PIO_cap`: highest PIO mode supported.
    pub PIO_cap: Cell<u8>,
    /// `DMA_cap`: highest DMA mode supported.
    pub DMA_cap: Cell<u8>,
    /// `UDMA_cap`: highest UDMA mode supported.
    pub UDMA_cap: Cell<u8>,
    /// `nchannels`: number of channels on this controller.
    pub nchannels: Cell<i32>,
    /// `channels`: channel-specific data (array).
    pub channels: Cell<Option<&'static [WdcChannelPtr]>>,
    /// `quirks`: per-device oddities (`WDC_QUIRK_*`).
    pub quirks: Cell<u16>,

    // #if 0: struct scsipi_adapter sc_atapi_adapter (the reference count for both IDE and
    // ATAPI devices).

    /* if WDC_CAPABILITY_DMA set in 'cap' */
    /// `dma_arg`.
    pub dma_arg: Cell<*mut c_void>,
    /// `dma_init`.
    pub dma_init: Cell<Option<WdcDmaInitFn>>,
    /// `dma_start`.
    pub dma_start: Cell<Option<WdcDmaStartFn>>,
    /// `dma_finish`.
    pub dma_finish: Cell<Option<WdcDmaFinishFn>>,
    /// `dma_status`: status return from `dma_finish()` (`WDC_DMAST_*`).
    pub dma_status: Cell<i32>,

    /* if WDC_CAPABILITY_MODE set in 'cap' */
    /// `set_modes`.
    pub set_modes: Cell<Option<fn(&ChannelSoftc)>>,

    /* if WDC_CAPABILITY_IRQACK set in 'cap' */
    /// `irqack`.
    pub irqack: Cell<Option<fn(&ChannelSoftc)>>,

    /// `reset`.
    pub reset: Cell<Option<fn(&ChannelSoftc)>>,

    /// `drv_probe`: driver callback to probe for drives.
    pub drv_probe: Cell<Option<fn(&ChannelSoftc)>>,
}

impl WdcSoftc {
    /// `wdc->cap & f`.
    pub fn has(&self, f: i32) -> bool {
        self.cap.get() & f != 0
    }

    /// `(*wdc->dma_finish)(wdc->dma_arg, channel, drive, force)`. Panics where the C would
    /// call through NULL.
    pub fn dma_finish(&self, channel: i32, drive: i32, force: i32) -> i32 {
        let Some(f) = self.dma_finish.get() else {
            panic(format_args!("{}: no dma_finish", self.sc_dev.xname()));
        };
        // SAFETY: the front-end pairs `dma_finish` with its own `dma_arg`.
        unsafe { f(self.dma_arg.get(), channel, drive, force) }
    }

    /// `(*wdc->dma_start)(wdc->dma_arg, channel, drive)`.
    pub fn dma_start(&self, channel: i32, drive: i32) {
        let Some(f) = self.dma_start.get() else {
            panic(format_args!("{}: no dma_start", self.sc_dev.xname()));
        };
        // SAFETY: the front-end pairs `dma_start` with its own `dma_arg`.
        unsafe { f(self.dma_arg.get(), channel, drive) }
    }

    /// `(*wdc->dma_init)(wdc->dma_arg, channel, drive, databuf, datalen, flags)`.
    ///
    /// # Safety
    ///
    /// `databuf` points to `datalen` bytes that stay valid until the transfer's
    /// `dma_finish`.
    pub unsafe fn dma_init(
        &self,
        channel: i32,
        drive: i32,
        databuf: *mut u8,
        datalen: usize,
        flags: i32,
    ) -> Result<(), Errno> {
        let Some(f) = self.dma_init.get() else {
            panic(format_args!("{}: no dma_init", self.sc_dev.xname()));
        };
        // SAFETY: the front-end pairs `dma_init` with its own `dma_arg`; the buffer is the
        // caller's contract.
        unsafe { f(self.dma_arg.get(), channel, drive, databuf, datalen, flags) }
    }
}

// SAFETY: `#[repr(C)]` with the device first; the other members are `Cell`s of integers,
// raw pointers, `Option`s of references, `NonNull`s and function pointers, all valid as all
// zero bits.
unsafe impl Softc for WdcSoftc {}

/// `struct atapi_return_args` (`atapiscsi.c`, not ported): opaque here.
pub struct AtapiReturnArgs {
    _private: [u8; 0],
}

/// The type of `c_start` and `c_kill_xfer`.
pub type WdcXferStartFn = fn(&ChannelSoftc, &WdcXfer);

/// The type of `c_intr`: 1 when the interrupt was for the transfer, 0 when not, -1 when
/// the channel is gone.
pub type WdcXferIntrFn = fn(&ChannelSoftc, &WdcXfer, i32) -> i32;

/// The type of `next` and `c_done`, ATAPISCSI's continuations.
pub type WdcXferNextFn = fn(&ChannelSoftc, &WdcXfer, i32, Option<NonNull<AtapiReturnArgs>>);

/// `struct wdc_xfer`: description of a command to be handled by a controller. These
/// commands are queued in a list.
pub struct WdcXfer {
    /// `c_flags` (`C_*`; `volatile` in the C).
    pub c_flags: Cell<u32>,

    /* Information about our location */
    /// `chp`.
    pub chp: Cell<Option<NonNull<ChannelSoftc>>>,
    /// `drive`.
    pub drive: Cell<u8>,

    /* Information about the current transfer  */
    /// `cmd`: wdc, ata or scsipi command structure.
    pub cmd: Cell<*mut c_void>,
    /// `databuf`.
    pub databuf: Cell<*mut u8>,
    /// `c_bcount`: byte count left.
    pub c_bcount: Cell<i32>,
    /// `c_skip`: bytes already transferred.
    pub c_skip: Cell<i32>,
    /// `c_xferchain`.
    pub c_xferchain: TailqEntry<WdcXfer>,
    /// `free_list`.
    pub free_list: ListEntry<WdcXfer>,
    /// `c_start`.
    pub c_start: Cell<Option<WdcXferStartFn>>,
    /// `c_intr`.
    pub c_intr: Cell<Option<WdcXferIntrFn>>,
    /// `c_kill_xfer`.
    pub c_kill_xfer: Cell<Option<WdcXferStartFn>>,

    /* Used by ATAPISCSI */
    /// `endticks` (`volatile` in the C).
    pub endticks: Cell<i32>,
    /// `atapi_poll_to`.
    pub atapi_poll_to: Timeout,
    /// `next`.
    pub next: Cell<Option<WdcXferNextFn>>,
    /// `c_done`.
    pub c_done: Cell<Option<WdcXferNextFn>>,

    /* Used for tape devices */
    /// `transfer_len`.
    pub transfer_len: Cell<i32>,
}

impl WdcXfer {
    /// `xfer->c_flags & f`.
    pub fn isset(&self, f: u32) -> bool {
        self.c_flags.get() & f != 0
    }

    /// `xfer->c_flags |= f`.
    pub fn set(&self, f: u32) {
        self.c_flags.set(self.c_flags.get() | f);
    }

    /// `xfer->c_flags &= ~f`.
    pub fn clr(&self, f: u32) {
        self.c_flags.set(self.c_flags.get() & !f);
    }
}

/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_offsets_and_blocks() {
        assert_eq!(wdr_status.offset(), 7);
        assert!(wdr_status.isset(_WDC_RDONLY));
        assert_eq!(wdr_command.offset(), 7);
        assert!(wdr_command.isset(_WDC_WRONLY));
        assert!(wdr_ctlr.isset(_WDC_AUX));
        assert_eq!(wdr_ctlr.offset(), 0);
        assert_eq!(wdr_seccnt, wdr_ireason);
        assert_eq!(wdr_lba_hi.offset(), wdr_cyl_hi.offset());
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/wdcvar.h");
        let names = crate::reftest::assert_defines!(defs;
            WDC_OPTION_PROBE_VERBOSE, WDCF_ACTIVE, WDCF_ONESLAVE, WDCF_IRQ_WAIT, WDCF_DMA_WAIT,
            WDCF_VERBOSE_PROBE, WDCF_DMA_BEFORE_CMD, _WDC_REGMASK, _WDC_AUX, _WDC_RDONLY,
            _WDC_WRONLY, WDC_NREG, WDC_NSHADOWREG, WDC_CAPABILITY_DATA16, WDC_CAPABILITY_DATA32,
            WDC_CAPABILITY_MODE, WDC_CAPABILITY_DMA, WDC_CAPABILITY_UDMA,
            WDC_CAPABILITY_NO_EXTRA_RESETS, WDC_CAPABILITY_PREATA, WDC_CAPABILITY_IRQACK,
            WDC_CAPABILITY_SINGLE_DRIVE, WDC_CAPABILITY_NO_ATAPI_DMA, WDC_CAPABILITY_SATA,
            WDC_QUIRK_NOSHORTDMA, WDC_QUIRK_NOATA, WDC_QUIRK_NOATAPI, WDC_DMA_READ, WDC_DMA_IRQW,
            WDC_DMA_LBA48, WDC_DMAST_NOIRQ, WDC_DMAST_ERR, WDC_DMAST_UNDER, C_ATAPI, C_TIMEOU,
            C_NEEDDONE, C_POLL, C_DMA, C_SENSE, C_MEDIA_ACCESS, C_POLL_MACHINE, C_PRIVATEXFER,
            C_SCSIXFER, WDC_CANSLEEP, WDC_NOSLEEP, NOWAIT, VERBOSE, SILENT, WDC_RESET_WAIT,
        );
        for prefix in ["WDC", "_WDC_", "C_", "NOWAIT", "VERBOSE", "SILENT"] {
            crate::reftest::assert_complete(&defs, prefix, &names);
        }
    }
}
/* </TESTS> */
