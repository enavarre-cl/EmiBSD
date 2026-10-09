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
//! `<dev/ic/siopvar_common.h>`: the structures and routines siop(4) shares with esiop: the
//! transfer tables the SCRIPTS processor reads (`struct siop_common_xfer`), the command and
//! per-target state the driver keeps, the adapter's common softc and its feature bits.
//!
//! Upstream: sys/dev/ic/siopvar_common.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `struct siop_common_xfer` lives in DMA memory that the chip reads and writes behind the
//!   compiler's back. It is `#[repr(C)]` with every member a `Cell` (the C's packed layout
//!   is also the natural one: every member is at a multiple of its alignment, asserted at
//!   the end), and every access goes through [`dma_get`]/[`dma_set`], volatile reads and
//!   writes of the cell, so that no table store is moved past the register write that starts
//!   the chip and no byte the chip wrote is read from a stale register copy.
//! - `scr_table_t` is [`ScrTable`], two `u32`s (packed and natural layouts agree).
//! - `struct siop_common_cmd`, `struct siop_common_target` and `struct siop_common_softc`
//!   are `#[repr(C)]` structures of `Cell`s, valid as all-zero bytes (they are `malloc`ed
//!   with `M_ZERO` or are the head of a zeroed softc). Pointers the C may leave NULL are
//!   `Option`s; their accessors (`sc()`, `xs()`, `tables()`, `target()`, `dmamap()`, `rt()`,
//!   ...) panic where the C would dereference NULL.
//! - `targets[]` holds `NonNull<SiopCommonTarget>`, the head of siop's `struct siop_target`
//!   (`siopvar.rs`), which `siop_scsiprobe` allocates and `siop_scsifree` frees.
//! - `sc_reset` is an `Option` of a function on the common softc.
//! - The register accesses `bus_space_{read,write}_N(sc->sc_rt, sc->sc_rh, reg, ...)` that
//!   fill both C files are the methods `read_1`, `read_2`, `read_4`, `write_1` and
//!   `write_4` of the common softc.
//! - `siop_htoc32`/`siop_ctoh32` are functions of the common softc's `features`.
//! - The prototypes of `siop_common.c`'s functions are the functions of `siop_common.rs`;
//!   `SIOP_NEG_*` are `i32`s, the type the negotiation functions return.

use core::cell::Cell;
use core::ptr::{self, NonNull};

use crate::kern::subr_prf::panic;
use crate::machine::bus::{
    BusAddr, BusDmaTag, BusDmamap, BusSize, BusSpaceHandle, BusSpaceTag, bus_space_read_1,
    bus_space_read_2, bus_space_read_4, bus_space_write_1, bus_space_write_4,
};
use crate::scsi::scsi_all::{ScsiGeneric, ScsiSenseData};
use crate::scsi::scsiconf::ScsiXfer;
use crate::sys::device::Device;
use crate::sys::endian::{betoh32, htobe32, htole32, letoh32};
use crate::sys::param::PAGE_SIZE;

/// `SIOP_DEFAULT_TARGET`: the adapter's own target when the chip has none set.
pub const SIOP_DEFAULT_TARGET: u16 = 7;

/// `scr_table_t` (`struct scr_table`): a table the SCRIPTS read with table-indirect moves.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrTable {
    /// `count`.
    pub count: u32,
    /// `addr`.
    pub addr: u32,
}

/// `SIOP_NSG`: number of scatter/gather entries ("XXX Ensure alignment of siop_xfer's",
/// "XXX (MAXPHYS/PAGE_SIZE + 1)").
pub const SIOP_NSG: usize = 17;
/// `SIOP_MAXFER`.
pub const SIOP_MAXFER: usize = (SIOP_NSG - 1) * PAGE_SIZE;

/// `struct siop_common_xfer`: interfaces the SCRIPT with the driver; it describes a full
/// transfer. If you change something here, don't forget to update offsets in `siop.ss`
/// (the `A_t_*` constants of the microcode; asserted at the end).
#[repr(C)]
pub struct SiopCommonXfer {
    /// `msg_out` (0).
    pub msg_out: [Cell<u8>; 16],
    /// `msg_in` (16): written by the chip.
    pub msg_in: [Cell<u8>; 16],
    /// `status` (32): written by the chip.
    pub status: Cell<u32>,
    /// `pad1` (36).
    pub pad1: Cell<u32>,
    /// `id` (40).
    pub id: Cell<u32>,
    /// `xscmd` (44).
    pub xscmd: Cell<ScsiGeneric>,
    /// `t_msgin` (60).
    pub t_msgin: Cell<ScrTable>,
    /// `t_extmsgin` (68).
    pub t_extmsgin: Cell<ScrTable>,
    /// `t_extmsgdata` (76).
    pub t_extmsgdata: Cell<ScrTable>,
    /// `t_msgout` (84).
    pub t_msgout: Cell<ScrTable>,
    /// `cmd` (92).
    pub cmd: Cell<ScrTable>,
    /// `t_status` (100).
    pub t_status: Cell<ScrTable>,
    /// `data` (108).
    pub data: [Cell<ScrTable>; SIOP_NSG],
}

impl SiopCommonXfer {
    /// `tables->msg_in[i]`, as the chip left it.
    pub fn msg_in(&self, i: usize) -> u8 {
        dma_get(&self.msg_in[i])
    }

    /// `tables->msg_out[i]`.
    pub fn msg_out(&self, i: usize) -> u8 {
        dma_get(&self.msg_out[i])
    }

    /// `tables->msg_out[i] = v`.
    pub fn set_msg_out(&self, i: usize, v: u8) {
        dma_set(&self.msg_out[i], v);
    }

    /// `table.count = count`, the address unchanged.
    pub fn set_count(table: &Cell<ScrTable>, count: u32) {
        let mut t = dma_get(table);
        t.count = count;
        dma_set(table, t);
    }
}

/// `SCSI_SIOP_NOCHECK`: don't check the scsi status (an additional value of `status`).
pub const SCSI_SIOP_NOCHECK: u32 = 0xfe;
/// `SCSI_SIOP_NOSTATUS`: device didn't report status.
pub const SCSI_SIOP_NOSTATUS: u32 = 0xff;

/// `SIOP_NOOFFSET`: offset is initialised to this, used to check if it was updated.
pub const SIOP_NOOFFSET: u32 = 0xffff_ffff;

/// `struct siop_common_cmd`: a command handled by the SCSI controller.
#[repr(C)]
pub struct SiopCommonCmd {
    /// `siop_sc`: points back to our adapter.
    pub siop_sc: Cell<Option<&'static SiopCommonSoftc>>,
    /// `siop_target`: pointer to our target def.
    pub siop_target: Cell<Option<NonNull<SiopCommonTarget>>>,
    /// `xs`: xfer from the upper level.
    pub xs: Cell<Option<&'static ScsiXfer>>,
    /// `siop_tables`: tables for this cmd (in DMA memory).
    pub siop_tables: Cell<Option<&'static SiopCommonXfer>>,
    /// `sense`: the sense buffer (in DMA memory).
    pub sense: Cell<*mut ScsiSenseData>,
    /// `dsa`: DSA value to load.
    pub dsa: Cell<BusAddr>,
    /// `dmamap_data`.
    pub dmamap_data: Cell<Option<&'static BusDmamap>>,
    /// `status`: `CMDST_*`.
    pub status: Cell<i32>,
    /// `flags`: `CMDFL_*`.
    pub flags: Cell<i32>,
    /// `tag`: tag used for tagged command queuing.
    pub tag: Cell<i32>,
    /// `resid`: valid when `CMDFL_RESID` is set.
    pub resid: Cell<i32>,
}

impl SiopCommonCmd {
    /// `siop_cmd->siop_sc`, set when the command block is made.
    pub fn sc(&self) -> &'static SiopCommonSoftc {
        match self.siop_sc.get() {
            Some(sc) => sc,
            None => panic(format_args!("siop: command without an adapter")),
        }
    }

    /// `siop_cmd->xs`, set by `siop_scsicmd`.
    pub fn xs(&self) -> &'static ScsiXfer {
        match self.xs.get() {
            Some(xs) => xs,
            None => panic(format_args!("siop: command without a transfer")),
        }
    }

    /// `siop_cmd->siop_tables`, set when the command block is made.
    pub fn tables(&self) -> &'static SiopCommonXfer {
        match self.siop_tables.get() {
            Some(t) => t,
            None => panic(format_args!("siop: command without tables")),
        }
    }

    /// `siop_cmd->dmamap_data`, made with the command block.
    pub fn dmamap(&self) -> &'static BusDmamap {
        match self.dmamap_data.get() {
            Some(m) => m,
            None => panic(format_args!("siop: command without a data map")),
        }
    }

    /// `siop_cmd->siop_target`, set by `siop_scsicmd`.
    pub fn target(&self) -> &'static SiopCommonTarget {
        match self.siop_target.get() {
            // SAFETY: the target of the command's link, allocated by siop_scsiprobe; it is
            // freed by siop_scsifree only once the link (and so every command for it) is
            // gone.
            Some(t) => unsafe { &*t.as_ptr() },
            None => panic(format_args!("siop: command without a target")),
        }
    }
}

/// `CMDST_FREE`: cmd slot is free.
pub const CMDST_FREE: i32 = 0;
/// `CMDST_READY`: cmd slot is waiting for processing.
pub const CMDST_READY: i32 = 1;
/// `CMDST_ACTIVE`: cmd slot is being processed.
pub const CMDST_ACTIVE: i32 = 2;
/// `CMDST_SENSE`: cmd slot is requesting sense.
pub const CMDST_SENSE: i32 = 3;
/// `CMDST_SENSE_ACTIVE`: request sense active.
pub const CMDST_SENSE_ACTIVE: i32 = 4;
/// `CMDST_SENSE_DONE`: request sense done.
pub const CMDST_SENSE_DONE: i32 = 5;
/// `CMDST_DONE`: cmd slot has been processed.
pub const CMDST_DONE: i32 = 6;

/// `CMDFL_TIMEOUT`: cmd timed out.
pub const CMDFL_TIMEOUT: i32 = 0x0001;
/// `CMDFL_TAG`: tagged cmd.
pub const CMDFL_TAG: i32 = 0x0002;
/// `CMDFL_RESID`: current offset in table is partial.
pub const CMDFL_RESID: i32 = 0x0004;

/// `struct siop_common_target`: per-target state.
#[repr(C)]
pub struct SiopCommonTarget {
    /// `status`: target status, `TARST_*`.
    pub status: Cell<i32>,
    /// `flags`: target flags, `TARF_*`.
    pub flags: Cell<i32>,
    /// `id`: for SELECT FROM (`SCNTL3` in bits 24-31, the target in 16-23, `SXFER` in 8-15,
    /// `SCNTL4` in 0-7).
    pub id: Cell<u32>,
    /// `period`.
    pub period: Cell<i32>,
    /// `offset`.
    pub offset: Cell<i32>,
}

/// `TARST_PROBING`: target is being probed.
pub const TARST_PROBING: i32 = 0;
/// `TARST_ASYNC`: target needs sync/wide negotiation.
pub const TARST_ASYNC: i32 = 1;
/// `TARST_WIDE_NEG`: target is doing wide negotiation.
pub const TARST_WIDE_NEG: i32 = 2;
/// `TARST_SYNC_NEG`: target is doing sync negotiation.
pub const TARST_SYNC_NEG: i32 = 3;
/// `TARST_PPR_NEG`: target is doing sync negotiation.
pub const TARST_PPR_NEG: i32 = 4;
/// `TARST_OK`: sync/wide agreement is valid.
pub const TARST_OK: i32 = 5;

/// `TARF_SYNC`: target can do sync.
pub const TARF_SYNC: i32 = 0x01;
/// `TARF_WIDE`: target can do wide.
pub const TARF_WIDE: i32 = 0x02;
/// `TARF_TAG`: target can do tags.
pub const TARF_TAG: i32 = 0x04;
/// `TARF_DT`: target can do DT clocking.
pub const TARF_DT: i32 = 0x08;
/// `TARF_ISWIDE`: target is wide.
pub const TARF_ISWIDE: i32 = 0x10;
/// `TARF_ISDT`: target is doing DT clocking.
pub const TARF_ISDT: i32 = 0x20;

/// `struct siop_common_softc`: driver internal state.
#[repr(C)]
pub struct SiopCommonSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_id`: adapter's target on bus.
    pub sc_id: Cell<u16>,
    /// `features`: chip's features, `SF_*`.
    pub features: Cell<i32>,
    /// `ram_size`.
    pub ram_size: Cell<i32>,
    /// `maxburst`.
    pub maxburst: Cell<i32>,
    /// `maxoff`.
    pub maxoff: Cell<i32>,
    /// `clock_div`: async. clock divider (scntl3).
    pub clock_div: Cell<i32>,
    /// `clock_period`: clock period (ns * 10).
    pub clock_period: Cell<i32>,
    /// `st_minsync`: min and max sync period,
    pub st_minsync: Cell<i32>,
    /// `dt_minsync`.
    pub dt_minsync: Cell<i32>,
    /// `st_maxsync`: as sent in or PPR messages.
    pub st_maxsync: Cell<i32>,
    /// `dt_maxsync`.
    pub dt_maxsync: Cell<i32>,
    /// `mode`: current SE/LVD/HVD mode.
    pub mode: Cell<i32>,
    /// `sc_rt`: bus_space registers tag.
    pub sc_rt: Cell<Option<BusSpaceTag>>,
    /// `sc_rh`: bus_space registers handle.
    pub sc_rh: Cell<Option<BusSpaceHandle>>,
    /// `sc_raddr`: register addresses.
    pub sc_raddr: Cell<BusAddr>,
    /// `sc_ramt`: bus_space ram tag.
    pub sc_ramt: Cell<Option<BusSpaceTag>>,
    /// `sc_ramh`: bus_space ram handle.
    pub sc_ramh: Cell<Option<BusSpaceHandle>>,
    /// `sc_dmat`: bus DMA tag.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_reset`: reset callback.
    pub sc_reset: Cell<Option<fn(&SiopCommonSoftc)>>,
    /// `sc_scriptdma`: DMA map for script.
    pub sc_scriptdma: Cell<Option<&'static BusDmamap>>,
    /// `sc_scriptaddr`: on-board ram or physical address.
    pub sc_scriptaddr: Cell<BusAddr>,
    /// `sc_script`: script location in memory (`PAGE_SIZE / 4` words, without on-board
    /// RAM).
    pub sc_script: Cell<*mut u32>,
    /// `targets[]`: per-target states.
    pub targets: [Cell<Option<NonNull<SiopCommonTarget>>>; 16],
}

impl SiopCommonSoftc {
    /// `sc->sc_rt` and `sc->sc_rh`, which the bus front-end maps before `siop_attach`.
    pub fn regs(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.sc_rt.get(), self.sc_rh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!(
                "{}: registers not mapped",
                self.sc_dev.xname()
            )),
        }
    }

    /// `bus_space_read_1(sc->sc_rt, sc->sc_rh, reg)`.
    pub fn read_1(&self, reg: BusSize) -> u8 {
        let (t, h) = self.regs();
        bus_space_read_1(t, h, reg)
    }

    /// `bus_space_read_2(sc->sc_rt, sc->sc_rh, reg)`.
    pub fn read_2(&self, reg: BusSize) -> u16 {
        let (t, h) = self.regs();
        bus_space_read_2(t, h, reg)
    }

    /// `bus_space_read_4(sc->sc_rt, sc->sc_rh, reg)`.
    pub fn read_4(&self, reg: BusSize) -> u32 {
        let (t, h) = self.regs();
        bus_space_read_4(t, h, reg)
    }

    /// `bus_space_write_1(sc->sc_rt, sc->sc_rh, reg, v)`.
    pub fn write_1(&self, reg: BusSize, v: u8) {
        let (t, h) = self.regs();
        bus_space_write_1(t, h, reg, v);
    }

    /// `bus_space_write_4(sc->sc_rt, sc->sc_rh, reg, v)`.
    pub fn write_4(&self, reg: BusSize, v: u32) {
        let (t, h) = self.regs();
        bus_space_write_4(t, h, reg, v);
    }

    /// `sc->sc_ramt` and `sc->sc_ramh`, mapped when the chip has on-board RAM
    /// (`SF_CHIP_RAM`).
    pub fn ram(&self) -> (BusSpaceTag, BusSpaceHandle) {
        match (self.sc_ramt.get(), self.sc_ramh.get()) {
            (Some(t), Some(h)) => (t, h),
            _ => panic(format_args!("{}: RAM not mapped", self.sc_dev.xname())),
        }
    }

    /// `sc->sc_dmat`, which the bus front-end sets before `siop_attach`.
    pub fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => panic(format_args!("{}: no DMA tag", self.sc_dev.xname())),
        }
    }

    /// `sc->sc_scriptdma`, made by `siop_common_attach` without on-board RAM.
    pub fn scriptdma(&self) -> &'static BusDmamap {
        match self.sc_scriptdma.get() {
            Some(m) => m,
            None => panic(format_args!("{}: no script DMA map", self.sc_dev.xname())),
        }
    }

    /// `sc->targets[target]`, `None` for NULL.
    pub fn target_opt(&self, target: usize) -> Option<&'static SiopCommonTarget> {
        // SAFETY: a non-NULL entry is a target siop_scsiprobe allocated; siop_scsifree
        // clears the entry before it frees it, and both run under the kernel lock.
        self.targets[target].get().map(|t| unsafe { &*t.as_ptr() })
    }

    /// `sc->targets[target]`, which the C dereferences without a check.
    pub fn target(&self, target: usize) -> &'static SiopCommonTarget {
        match self.target_opt(target) {
            Some(t) => t,
            None => panic(format_args!(
                "{}: no state for target {target}",
                self.sc_dev.xname()
            )),
        }
    }

    /// `&sc->sc_script[i]`, the host copy of the script (without on-board RAM).
    pub fn script_word(&self, i: usize) -> *mut u32 {
        let script = self.sc_script.get();
        if script.is_null() || i >= PAGE_SIZE / 4 {
            panic(format_args!(
                "{}: script word {i} out of the script page",
                self.sc_dev.xname()
            ));
        }
        // SAFETY: siop_common_attach maps a page of DMA memory at `sc_script`, never
        // unmapped; `i` is inside it.
        unsafe { script.add(i) }
    }
}

/// `SF_BUS_WIDE`: wide bus.
pub const SF_BUS_WIDE: i32 = 0x00000001;
/// `SF_BUS_ULTRA`: Ultra (20MHz) bus.
pub const SF_BUS_ULTRA: i32 = 0x00000002;
/// `SF_BUS_ULTRA2`: Ultra2 (40MHz) bus.
pub const SF_BUS_ULTRA2: i32 = 0x00000004;
/// `SF_BUS_ULTRA3`: Ultra3 (80MHz) bus.
pub const SF_BUS_ULTRA3: i32 = 0x00000008;
/// `SF_BUS_DIFF`: differential bus.
pub const SF_BUS_DIFF: i32 = 0x00000010;

/// `SF_CHIP_LED0`: led on GPIO0.
pub const SF_CHIP_LED0: i32 = 0x00000100;
/// `SF_CHIP_LEDC`: led on GPIO0 with hardware control.
pub const SF_CHIP_LEDC: i32 = 0x00000200;
/// `SF_CHIP_DBLR`: clock doubler or quadrupler.
pub const SF_CHIP_DBLR: i32 = 0x00000400;
/// `SF_CHIP_QUAD`: clock quadrupler, with PPL.
pub const SF_CHIP_QUAD: i32 = 0x00000800;
/// `SF_CHIP_FIFO`: large fifo.
pub const SF_CHIP_FIFO: i32 = 0x00001000;
/// `SF_CHIP_PF`: Instructions prefetch.
pub const SF_CHIP_PF: i32 = 0x00002000;
/// `SF_CHIP_RAM`: on-board RAM.
pub const SF_CHIP_RAM: i32 = 0x00004000;
/// `SF_CHIP_LS`: load/store instruction.
pub const SF_CHIP_LS: i32 = 0x00008000;
/// `SF_CHIP_10REGS`: 10 scratch registers.
pub const SF_CHIP_10REGS: i32 = 0x00010000;
/// `SF_CHIP_DFBC`: Use DFBC register.
pub const SF_CHIP_DFBC: i32 = 0x00020000;
/// `SF_CHIP_DT`: DT clocking.
pub const SF_CHIP_DT: i32 = 0x00040000;
/// `SF_CHIP_GEBUG`: SCSI gross error bug.
pub const SF_CHIP_GEBUG: i32 = 0x00080000;
/// `SF_CHIP_AAIP`: Always generate AIP regardless of SNCTL4.
pub const SF_CHIP_AAIP: i32 = 0x00100000;
/// `SF_CHIP_BE`: big-endian.
pub const SF_CHIP_BE: i32 = 0x00200000;

/// `SF_PCI_RL`: PCI read line.
pub const SF_PCI_RL: i32 = 0x01000000;
/// `SF_PCI_RM`: PCI read multiple.
pub const SF_PCI_RM: i32 = 0x02000000;
/// `SF_PCI_BOF`: PCI burst opcode fetch.
pub const SF_PCI_BOF: i32 = 0x04000000;
/// `SF_PCI_CLS`: PCI cache line size.
pub const SF_PCI_CLS: i32 = 0x08000000;
/// `SF_PCI_WRI`: PCI write and invalidate.
pub const SF_PCI_WRI: i32 = 0x10000000;

/// `SIOP_NEG_NOP`: action to take at return of `siop_wdtr_neg()` and `siop_sdtr_neg()`.
pub const SIOP_NEG_NOP: i32 = 0x0;
/// `SIOP_NEG_MSGOUT`.
pub const SIOP_NEG_MSGOUT: i32 = 0x1;
/// `SIOP_NEG_ACK`.
pub const SIOP_NEG_ACK: i32 = 0x2;

/// A volatile read of a cell of the DMA tables (see the module's deviations).
pub fn dma_get<T: Copy>(c: &Cell<T>) -> T {
    // SAFETY: the cell's own bytes, aligned for `T` (`repr(C)` at an aligned address), and
    // any bytes the chip writes are a valid `T` (integers, byte arrays).
    unsafe { ptr::read_volatile(c.as_ptr()) }
}

/// A volatile write of a cell of the DMA tables (see the module's deviations).
pub fn dma_set<T: Copy>(c: &Cell<T>, v: T) {
    // SAFETY: the cell's own bytes, aligned for `T`; a `Cell` may be written through a
    // shared reference.
    unsafe { ptr::write_volatile(c.as_ptr(), v) }
}

/// `siop_htoc32(sc, x)`: host to chip byte order.
pub fn siop_htoc32(sc: &SiopCommonSoftc, x: u32) -> u32 {
    if sc.features.get() & SF_CHIP_BE != 0 {
        htobe32(x)
    } else {
        htole32(x)
    }
}

/// `siop_ctoh32(sc, x)`: chip to host byte order.
pub fn siop_ctoh32(sc: &SiopCommonSoftc, x: u32) -> u32 {
    if sc.features.get() & SF_CHIP_BE != 0 {
        betoh32(x)
    } else {
        letoh32(x)
    }
}

const _: () = {
    use core::mem::offset_of;

    use crate::dev::microcode::siop::siop::{
        A_t_cmd, A_t_data, A_t_ext_msg_data, A_t_ext_msg_in, A_t_id, A_t_msg_in, A_t_msg_out,
        A_t_status,
    };

    assert!(size_of::<ScrTable>() == 8);
    assert!(offset_of!(SiopCommonXfer, msg_in) == 16);
    assert!(offset_of!(SiopCommonXfer, status) == 32);
    assert!(offset_of!(SiopCommonXfer, id) == A_t_id as usize);
    assert!(offset_of!(SiopCommonXfer, xscmd) == 44);
    assert!(offset_of!(SiopCommonXfer, t_msgin) == A_t_msg_in as usize);
    assert!(offset_of!(SiopCommonXfer, t_extmsgin) == A_t_ext_msg_in as usize);
    assert!(offset_of!(SiopCommonXfer, t_extmsgdata) == A_t_ext_msg_data as usize);
    assert!(offset_of!(SiopCommonXfer, t_msgout) == A_t_msg_out as usize);
    assert!(offset_of!(SiopCommonXfer, cmd) == A_t_cmd as usize);
    assert!(offset_of!(SiopCommonXfer, t_status) == A_t_status as usize);
    assert!(offset_of!(SiopCommonXfer, data) == A_t_data as usize);
    assert!(size_of::<SiopCommonXfer>() == 108 + 8 * SIOP_NSG);
};
/* </CODE> */
