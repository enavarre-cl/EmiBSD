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
 * Copyright (c) 2006 Uwe Stuehler <uwe@openbsd.org>
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

/* <CODE> */
//! `<dev/sdmmc/sdmmcvar.h>`: the SD/MMC bus state: one `sdmmc_softc` per card slot, one
//! `sdmmc_function` per memory card or SDIO function found in it, the commands sent to the
//! cards, the slot's task queue, and the decoded CSD, CID, SCR and CIS.
//!
//! Upstream: sys/dev/sdmmc/sdmmcvar.h @ 3ce1f3f79392
//!
//! The functions declared here live in `sdmmc.rs`, `sdmmc_io.rs`, `sdmmc_cis.rs` and
//! `sdmmc_mem.rs`, the modules of the C files that define them.
//!
//! ## Deviations
//! - The softc is `#[repr(C)]` with the device first and reached as `&'static`; its members
//!   are `Cell`s, all-zero valid. `sct` is an `Option` (set by attach), `sc_card` and
//!   `sc_fn0` raw pointers to functions on `sf_head`, `sc_scsibus` the C's `void *`.
//! - `struct sdmmc_function` is allocated whole by `sdmmc_function_alloc` (its `sc` is set
//!   there and never changes); the decoded registers (`csd`, `cid`, `raw_cid`, `scr`, `cis`)
//!   are `Cell`s of `Copy` structures, read and written whole, under the slot's `sc_lock`.
//! - `cis1_info[]` of `struct sdmmc_cis` names its strings by [`Cis1Info`]: an offset into
//!   `cis1_info_buf` or a static string, where the C keeps pointers into the structure
//!   itself (or to string literals); the structure stays `Copy`.
//! - `struct sdmmc_command` is a plain structure on the caller's stack, as in C, passed by
//!   `&mut` to the chip's `exec_command`; `c_error` is an `Option<Errno>` (`None` for 0).
//! - `struct sdmmc_attach_args` is `#[repr(C)]` with the `scsibus_attach_args` first, so
//!   `scsibus(4)` reads it as its own attach arguments, as in C.
//! - The ioctl structures and numbers are defined (SDMMC_IOCTL is not configured, so nothing
//!   uses them); the numbers are built from the C structures' LP64 sizes (128 and 16
//!   bytes), not from the Rust ones.
//! - The `sdmmc_init_task`, `sdmmc_task_pending`, `splsdmmc` and `SDMMC_ASSERT_LOCKED` macros
//!   are functions.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::sdmmc::sdmmc_io::SdmmcIntrHandlerList;
use crate::dev::sdmmc::sdmmcchip::{SDMMC_MAX_FUNCTIONS, SdmmcChipsetHandle, SdmmcChipsetTag};
use crate::kern::kern_rwlock::rw_assert_wrlock;
use crate::kern::subr_prf::panic;
use crate::machine::bus::{BusDmaTag, BusDmamap};
use crate::machine::intr::{IPL_BIO, splbio};
use crate::queue_adapter;
use crate::scsi::scsiconf::ScsibusAttachArgs;
use crate::sys::device::{Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::ioccom::{_ioc, IOC_INOUT};
use crate::sys::param::{MAXPHYS, PAGE_SIZE};
use crate::sys::proc::Proc;
use crate::sys::queue::{SimpleqEntry, SimpleqHead, TailqEntry, TailqHead};
use crate::sys::rwlock::Rwlock;

/// `SCF_ITSDONE`: command is complete.
pub const SCF_ITSDONE: i32 = 0x0001;
/// `SCF_CMD_AC`.
pub const SCF_CMD_AC: i32 = 0x0000;
/// `SCF_CMD_ADTC`.
pub const SCF_CMD_ADTC: i32 = 0x0010;
/// `SCF_CMD_BC`.
pub const SCF_CMD_BC: i32 = 0x0020;
/// `SCF_CMD_BCR`.
pub const SCF_CMD_BCR: i32 = 0x0030;
/// `SCF_CMD_READ`: read command (data expected).
pub const SCF_CMD_READ: i32 = 0x0040;
/// `SCF_RSP_BSY`.
pub const SCF_RSP_BSY: i32 = 0x0100;
/// `SCF_RSP_136`.
pub const SCF_RSP_136: i32 = 0x0200;
/// `SCF_RSP_CRC`.
pub const SCF_RSP_CRC: i32 = 0x0400;
/// `SCF_RSP_IDX`.
pub const SCF_RSP_IDX: i32 = 0x0800;
/// `SCF_RSP_PRESENT`.
pub const SCF_RSP_PRESENT: i32 = 0x1000;
/* response types */
/// `SCF_RSP_R0`: none.
pub const SCF_RSP_R0: i32 = 0;
/// `SCF_RSP_R1`.
pub const SCF_RSP_R1: i32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_IDX;
/// `SCF_RSP_R1B`.
pub const SCF_RSP_R1B: i32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_IDX | SCF_RSP_BSY;
/// `SCF_RSP_R2`.
pub const SCF_RSP_R2: i32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_136;
/// `SCF_RSP_R3`.
pub const SCF_RSP_R3: i32 = SCF_RSP_PRESENT;
/// `SCF_RSP_R4`.
pub const SCF_RSP_R4: i32 = SCF_RSP_PRESENT;
/// `SCF_RSP_R5`.
pub const SCF_RSP_R5: i32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_IDX;
/// `SCF_RSP_R5B`.
pub const SCF_RSP_R5B: i32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_IDX | SCF_RSP_BSY;
/// `SCF_RSP_R6`.
pub const SCF_RSP_R6: i32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_IDX;
/// `SCF_RSP_R7`.
pub const SCF_RSP_R7: i32 = SCF_RSP_PRESENT | SCF_RSP_CRC | SCF_RSP_IDX;

/// `SDMMC_VENDOR_INVALID`.
pub const SDMMC_VENDOR_INVALID: u16 = 0xffff;
/// `SDMMC_PRODUCT_INVALID`.
pub const SDMMC_PRODUCT_INVALID: u16 = 0xffff;
/// `SDMMC_FUNCTION_INVALID`.
pub const SDMMC_FUNCTION_INVALID: u8 = 0xff;

/// `SFF_ERROR`: function is poo; ignore it.
pub const SFF_ERROR: i32 = 0x0001;
/// `SFF_SDHC`: SD High Capacity card.
pub const SFF_SDHC: i32 = 0x0002;

/// `SDMMC_MAXNSEGS`.
pub const SDMMC_MAXNSEGS: usize = (MAXPHYS / PAGE_SIZE) + 1;

/// `SMF_SD_MODE`: host in SD mode (MMC otherwise).
pub const SMF_SD_MODE: i32 = 0x0001;
/// `SMF_IO_MODE`: host in I/O mode (SD mode only).
pub const SMF_IO_MODE: i32 = 0x0002;
/// `SMF_MEM_MODE`: host in memory mode (SD or MMC).
pub const SMF_MEM_MODE: i32 = 0x0004;
/// `SMF_UHS_MODE`: host in UHS mode.
pub const SMF_UHS_MODE: i32 = 0x0010;
/// `SMF_CARD_PRESENT`: card presence noticed.
pub const SMF_CARD_PRESENT: i32 = 0x0020;
/// `SMF_CARD_ATTACHED`: card driver(s) attached.
pub const SMF_CARD_ATTACHED: i32 = 0x0040;
/// `SMF_STOP_AFTER_MULTIPLE`: send a stop after a multiple cmd.
pub const SMF_STOP_AFTER_MULTIPLE: i32 = 0x0080;
/// `SMF_CONFIG_PENDING`: config_pending_incr() called.
pub const SMF_CONFIG_PENDING: i32 = 0x0100;

/// `SMC_CAPS_AUTO_STOP`: send CMD12 automagically by host.
pub const SMC_CAPS_AUTO_STOP: u32 = 0x0001;
/// `SMC_CAPS_4BIT_MODE`: 4-bits data bus width.
pub const SMC_CAPS_4BIT_MODE: u32 = 0x0002;
/// `SMC_CAPS_DMA`: DMA transfer.
pub const SMC_CAPS_DMA: u32 = 0x0004;
/// `SMC_CAPS_SPI_MODE`: SPI mode.
pub const SMC_CAPS_SPI_MODE: u32 = 0x0008;
/// `SMC_CAPS_POLL_CARD_DET`: polling card detect.
pub const SMC_CAPS_POLL_CARD_DET: u32 = 0x0010;
/// `SMC_CAPS_SINGLE_ONLY`: only single read/write.
pub const SMC_CAPS_SINGLE_ONLY: u32 = 0x0020;
/// `SMC_CAPS_8BIT_MODE`: 8-bits data bus width.
pub const SMC_CAPS_8BIT_MODE: u32 = 0x0040;
/// `SMC_CAPS_MULTI_SEG_DMA`: multiple segment DMA transfer.
pub const SMC_CAPS_MULTI_SEG_DMA: u32 = 0x0080;
/// `SMC_CAPS_SD_HIGHSPEED`: SD high-speed timing.
pub const SMC_CAPS_SD_HIGHSPEED: u32 = 0x0100;
/// `SMC_CAPS_MMC_HIGHSPEED`: MMC high-speed timing.
pub const SMC_CAPS_MMC_HIGHSPEED: u32 = 0x0200;
/// `SMC_CAPS_UHS_SDR50`: UHS SDR50 timing.
pub const SMC_CAPS_UHS_SDR50: u32 = 0x0400;
/// `SMC_CAPS_UHS_SDR104`: UHS SDR104 timing.
pub const SMC_CAPS_UHS_SDR104: u32 = 0x0800;
/// `SMC_CAPS_UHS_DDR50`: UHS DDR50 timing.
pub const SMC_CAPS_UHS_DDR50: u32 = 0x1000;
/// `SMC_CAPS_UHS_MASK`.
pub const SMC_CAPS_UHS_MASK: u32 = 0x1c00;
/// `SMC_CAPS_MMC_DDR52`: eMMC DDR52 timing.
pub const SMC_CAPS_MMC_DDR52: u32 = 0x2000;
/// `SMC_CAPS_MMC_HS200`: eMMC HS200 timing.
pub const SMC_CAPS_MMC_HS200: u32 = 0x4000;
/// `SMC_CAPS_MMC_HS400`: eMMC HS400 timing.
pub const SMC_CAPS_MMC_HS400: u32 = 0x8000;
/// `SMC_CAPS_NONREMOVABLE`: non-removable devices.
pub const SMC_CAPS_NONREMOVABLE: u32 = 0x10000;

/// `IPL_SDMMC`.
pub const IPL_SDMMC: i32 = IPL_BIO;

/// The size of the C's `struct bio_sdmmc_command` on LP64 (a pointer and a 120-byte
/// `struct sdmmc_command`).
const BIO_SDMMC_COMMAND_SIZE: usize = 128;
/// The size of the C's `struct bio_sdmmc_debug` on LP64.
const BIO_SDMMC_DEBUG_SIZE: usize = 16;

/// `SDIOCEXECMMC`: `_IOWR('S', 0, struct bio_sdmmc_command)`.
pub const SDIOCEXECMMC: u64 = _ioc(IOC_INOUT, b'S', 0, BIO_SDMMC_COMMAND_SIZE);
/// `SDIOCEXECAPP`: `_IOWR('S', 1, struct bio_sdmmc_command)`.
pub const SDIOCEXECAPP: u64 = _ioc(IOC_INOUT, b'S', 1, BIO_SDMMC_COMMAND_SIZE);
/// `SDIOCSETDEBUG`: `_IOWR('S', 2, struct bio_sdmmc_debug)`.
pub const SDIOCSETDEBUG: u64 = _ioc(IOC_INOUT, b'S', 2, BIO_SDMMC_DEBUG_SIZE);

/// `struct sdmmc_csd`.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SdmmcCsd {
    /// `csdver`: CSD structure format.
    pub csdver: i32,
    /// `mmcver`: MMC version (for CID format).
    pub mmcver: i32,
    /// `capacity`: total number of sectors.
    pub capacity: i32,
    /// `sector_size`: sector size in bytes.
    pub sector_size: i32,
    /// `read_bl_len`: block length for reads.
    pub read_bl_len: i32,
    /// `tran_speed`: transfer speed (kbit/s).
    pub tran_speed: i32,
    /// `ccc`: Card Command Class for SD.
    pub ccc: i32,
}

/// `struct sdmmc_cid`.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SdmmcCid {
    /// `mid`: manufacturer identification number.
    pub mid: i32,
    /// `oid`: OEM/product identification number.
    pub oid: i32,
    /// `pnm`: product name (MMC v1 has the longest).
    pub pnm: [u8; 8],
    /// `rev`: product revision.
    pub rev: i32,
    /// `psn`: product serial number.
    pub psn: i32,
    /// `mdt`: manufacturing date.
    pub mdt: i32,
}

/// `struct sdmmc_scr`.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct SdmmcScr {
    /// `sd_spec`.
    pub sd_spec: i32,
    /// `bus_width`.
    pub bus_width: i32,
}

/// `sdmmc_response`.
pub type SdmmcResponse = [u32; 4];

/// `struct sdmmc_task`: a function the slot's task thread runs.
pub struct SdmmcTask {
    /// `func`.
    pub func: Cell<Option<fn(*mut c_void)>>,
    /// `arg`.
    pub arg: Cell<*mut c_void>,
    /// `onqueue`.
    pub onqueue: Cell<i32>,
    /// `sc`: the slot whose queue holds the task, or null.
    pub sc: Cell<*const SdmmcSoftc>,
    /// `next`: the link in `sc_tskq`.
    pub next: TailqEntry<SdmmcTask>,
}

impl SdmmcTask {
    /// A task that is on no queue and runs nothing (all zero).
    pub const fn new() -> Self {
        Self {
            func: Cell::new(None),
            arg: Cell::new(ptr::null_mut()),
            onqueue: Cell::new(0),
            sc: Cell::new(ptr::null()),
            next: TailqEntry::new(),
        }
    }
}

impl Default for SdmmcTask {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(, sdmmc_task)`, through `next`.
    pub SdmmcTaskList: SdmmcTask, next => TailqEntry<SdmmcTask>
);

/// `struct sdmmc_command`.
pub struct SdmmcCommand {
    /// `c_task`: task queue entry.
    pub c_task: SdmmcTask,
    /// `c_opcode`: SD or MMC command index.
    pub c_opcode: u16,
    /// `c_arg`: SD/MMC command argument.
    pub c_arg: u32,
    /// `c_resp`: response buffer.
    pub c_resp: SdmmcResponse,
    /// `c_dmamap`.
    pub c_dmamap: Option<&'static BusDmamap>,
    /// `c_data`: buffer to send or read into.
    pub c_data: *mut u8,
    /// `c_datalen`: length of data buffer.
    pub c_datalen: i32,
    /// `c_blklen`: block length.
    pub c_blklen: i32,
    /// `c_flags`: `SCF_*`.
    pub c_flags: i32,
    /// `c_error`: errno value on completion.
    pub c_error: Option<Errno>,
    /// `c_resid`: remaining I/O (host controller owned, data transfer in progress).
    pub c_resid: i32,
    /// `c_buf`: remaining data (host controller owned).
    pub c_buf: *mut u8,
}

impl SdmmcCommand {
    /// A zeroed command (`bzero(&cmd, sizeof cmd)`).
    pub const fn new() -> Self {
        Self {
            c_task: SdmmcTask::new(),
            c_opcode: 0,
            c_arg: 0,
            c_resp: [0; 4],
            c_dmamap: None,
            c_data: ptr::null_mut(),
            c_datalen: 0,
            c_blklen: 0,
            c_flags: 0,
            c_error: None,
            c_resid: 0,
            c_buf: ptr::null_mut(),
        }
    }
}

impl Default for SdmmcCommand {
    fn default() -> Self {
        Self::new()
    }
}

/// One string of `cis1_info[]`: where the C keeps a `char *`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cis1Info {
    /// A NUL-terminated string at this offset of `cis1_info_buf`.
    Buf(usize),
    /// A string literal (`sdmmc_check_cis_quirks`).
    Static(&'static [u8]),
}

/// `struct sdmmc_cis`: decoded PC Card 16 based Card Information Structure (CIS), per card
/// (function 0) and per function (1 and greater).
#[derive(Clone, Copy, Debug)]
pub struct SdmmcCis {
    /// `manufacturer`.
    pub manufacturer: u16,
    /// `product`.
    pub product: u16,
    /// `function`.
    pub function: u8,
    /// `cis1_major`.
    pub cis1_major: u8,
    /// `cis1_minor`.
    pub cis1_minor: u8,
    /// `cis1_info_buf`.
    pub cis1_info_buf: [u8; 256],
    /// `cis1_info`.
    pub cis1_info: [Option<Cis1Info>; 4],
}

impl SdmmcCis {
    /// A zeroed CIS.
    pub const fn new() -> Self {
        Self {
            manufacturer: 0,
            product: 0,
            function: 0,
            cis1_major: 0,
            cis1_minor: 0,
            cis1_info_buf: [0; 256],
            cis1_info: [None; 4],
        }
    }

    /// `cis1_info[i]` as the bytes before its NUL, `None` for the C's NULL.
    pub fn info(&self, i: usize) -> Option<&[u8]> {
        match self.cis1_info.get(i).copied().flatten()? {
            Cis1Info::Buf(start) => {
                let s = self.cis1_info_buf.get(start..).unwrap_or(&[]);
                Some(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())])
            }
            Cis1Info::Static(s) => Some(s),
        }
    }
}

impl Default for SdmmcCis {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct sdmmc_function`: either an SD card I/O function or a SD/MMC memory card from a
/// "stack of cards" that responded to CMD2. For a combo card with one I/O function and one
/// memory card, there will be two of these structures allocated. Each card slot has such a
/// list of `sdmmc_function` structures.
pub struct SdmmcFunction {
    /* common members */
    /// `sc`: card slot softc.
    pub sc: &'static SdmmcSoftc,
    /// `rca`: relative card address.
    pub rca: Cell<u16>,
    /// `flags`: `SFF_*`.
    pub flags: Cell<i32>,
    /// `cookie`: pass extra info from bus to dev.
    pub cookie: Cell<*mut c_void>,
    /// `sf_list`: the link in `sf_head`.
    pub sf_list: SimpleqEntry<SdmmcFunction>,
    /* SD card I/O function members */
    /// `number`: I/O function number or -1.
    pub number: Cell<i32>,
    /// `child`: function driver.
    pub child: Cell<Option<NonNull<Device>>>,
    /// `cis`: decoded CIS.
    pub cis: Cell<SdmmcCis>,
    /// `cur_blklen`: current block length.
    pub cur_blklen: Cell<u32>,
    /* SD/MMC memory card members */
    /// `csd`: decoded CSD value.
    pub csd: Cell<SdmmcCsd>,
    /// `cid`: decoded CID value.
    pub cid: Cell<SdmmcCid>,
    /// `raw_cid`: temp. storage for decoding.
    pub raw_cid: Cell<SdmmcResponse>,
    /// `scr`: decoded SCR value.
    pub scr: Cell<SdmmcScr>,
}

queue_adapter!(
    /// `SIMPLEQ_HEAD(, sdmmc_function)`, through `sf_list`.
    pub SdmmcFunctionList: SdmmcFunction, sf_list => SimpleqEntry<SdmmcFunction>
);

/// `struct sdmmc_softc`: a single SD/MMC/SDIO card slot.
#[repr(C)]
pub struct SdmmcSoftc {
    /// `sc_dev`: base device.
    pub sc_dev: Device,
    /// `sct`: host controller chipset tag.
    pub sct: Cell<Option<SdmmcChipsetTag>>,
    /// `sch`: host controller chipset handle.
    pub sch: Cell<SdmmcChipsetHandle>,

    /// `sc_dmat`.
    pub sc_dmat: Cell<Option<BusDmaTag>>,
    /// `sc_dmap`.
    pub sc_dmap: Cell<Option<&'static BusDmamap>>,

    /// `sc_flags`: `SMF_*`.
    pub sc_flags: Cell<i32>,

    /// `sc_caps`: host capability (`SMC_CAPS_*`).
    pub sc_caps: Cell<u32>,

    /// `sc_function_count`: number of I/O functions (SDIO).
    pub sc_function_count: Cell<i32>,
    /// `sc_card`: selected card.
    pub sc_card: Cell<*const SdmmcFunction>,
    /// `sc_fn0`: function 0, the card itself.
    pub sc_fn0: Cell<*const SdmmcFunction>,
    /// `sf_head`: list of card functions.
    pub sf_head: SimpleqHead<SdmmcFunctionList>,
    /// `sc_dying`: bus driver is shutting down.
    pub sc_dying: Cell<i32>,
    /// `sc_task_thread`: asynchronous tasks.
    pub sc_task_thread: Cell<Option<&'static Proc>>,
    /// `sc_tskq`: task thread work queue.
    pub sc_tskq: TailqHead<SdmmcTaskList>,
    /// `sc_discover_task`: card attach/detach task.
    pub sc_discover_task: SdmmcTask,
    /// `sc_intr_task`: card interrupt task.
    pub sc_intr_task: SdmmcTask,
    /// `sc_lock`: lock around host controller.
    pub sc_lock: Rwlock,
    /// `sc_scsibus`: SCSI bus emulation softc (`sdmmc_scsi.rs`'s `SdmmcScsiSoftc`).
    pub sc_scsibus: Cell<*mut c_void>,
    /// `sc_intrq`: interrupt handlers.
    pub sc_intrq: TailqHead<SdmmcIntrHandlerList>,
    /// `sc_max_seg`: maximum segment size.
    pub sc_max_seg: Cell<i64>,
    /// `sc_max_xfer`: maximum transfer size.
    pub sc_max_xfer: Cell<i64>,
    /// `sc_cookies`: pass extra info from bus to dev.
    pub sc_cookies: Cell<[*mut c_void; SDMMC_MAX_FUNCTIONS]>,
}

impl SdmmcSoftc {
    /// `DEVNAME(sc)`.
    pub fn devname(&self) -> &str {
        self.sc_dev.xname()
    }

    /// `sc->sct`, which attach sets first.
    pub fn sct(&self) -> SdmmcChipsetTag {
        match self.sct.get() {
            Some(t) => t,
            None => panic(format_args!("sdmmc: no chipset tag")),
        }
    }

    /// `sc->sc_dmat`, which attach sets first.
    pub fn dmat(&self) -> BusDmaTag {
        match self.sc_dmat.get() {
            Some(t) => t,
            None => panic(format_args!("sdmmc: no DMA tag")),
        }
    }

    /// `sc->sc_dmap`, which attach creates when the host does DMA.
    pub fn dmap(&self) -> &'static BusDmamap {
        match self.sc_dmap.get() {
            Some(m) => m,
            None => panic(format_args!("sdmmc: no DMA map")),
        }
    }

    /// `sc->sc_fn0`: function 0, which every attached card has.
    pub fn fn0(&self) -> &'static SdmmcFunction {
        let f = self.sc_fn0.get();
        if f.is_null() {
            panic(format_args!("sdmmc: no function 0"));
        }
        // SAFETY: `sc_fn0` points to a function on `sf_head`, freed only by
        // `sdmmc_card_detach`, which clears it first under `sc_lock`.
        unsafe { &*f }
    }

    /// `ISSET(sc->sc_flags, f)`.
    pub fn has_flag(&self, f: i32) -> bool {
        self.sc_flags.get() & f != 0
    }

    /// `SET(sc->sc_flags, f)`.
    pub fn set_flag(&self, f: i32) {
        self.sc_flags.set(self.sc_flags.get() | f);
    }

    /// `CLR(sc->sc_flags, f)`.
    pub fn clr_flag(&self, f: i32) {
        self.sc_flags.set(self.sc_flags.get() & !f);
    }

    /// `ISSET(sc->sc_caps, c)`.
    pub fn has_caps(&self, c: u32) -> bool {
        self.sc_caps.get() & c != 0
    }
}

// SAFETY: `#[repr(C)]` with the device first; the queue heads, the tasks and the rwlock are
// all-zero valid (`sys/queue.rs`, `sys/rwlock.rs`), and every other member is a `Cell` of an
// integer, a raw pointer, or an `Option` of a reference or a tag.
unsafe impl Softc for SdmmcSoftc {}

/// `struct sdmmc_attach_args`: what attaches at the sdmmc bus.
#[repr(C)]
pub struct SdmmcAttachArgs {
    /// `saa`: the SCSI bus emulation's attach arguments.
    pub saa: ScsibusAttachArgs,
    /// `sf`: the SDIO function, `None` for the SCSI bus.
    pub sf: Option<&'static SdmmcFunction>,
}

/// `struct bio_sdmmc_command`.
pub struct BioSdmmcCommand {
    /// `cookie`.
    pub cookie: *mut c_void,
    /// `cmd`.
    pub cmd: SdmmcCommand,
}

/// `struct bio_sdmmc_debug`.
pub struct BioSdmmcDebug {
    /// `cookie`.
    pub cookie: *mut c_void,
    /// `debug`.
    pub debug: i32,
}

/// `SCF_CMD(flags)`.
pub const fn scf_cmd(flags: i32) -> i32 {
    flags & 0x00f0
}

/// `sdmmc_init_task(xtask, xfunc, xarg)`.
pub fn sdmmc_init_task(xtask: &SdmmcTask, xfunc: fn(*mut c_void), xarg: *mut c_void) {
    xtask.func.set(Some(xfunc));
    xtask.arg.set(xarg);
    xtask.onqueue.set(0);
    xtask.sc.set(ptr::null());
}

/// `sdmmc_task_pending(xtask)`.
pub fn sdmmc_task_pending(xtask: &SdmmcTask) -> bool {
    xtask.onqueue.get() != 0
}

/// `splsdmmc()`.
pub fn splsdmmc() -> i32 {
    splbio()
}

/// `SDMMC_ASSERT_LOCKED(sc)`.
pub fn sdmmc_assert_locked(sc: &SdmmcSoftc) {
    rw_assert_wrlock(&sc.sc_lock);
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest;

    #[test]
    fn cis_strings_come_from_the_buffer_or_a_literal() {
        let mut cis = SdmmcCis::new();
        cis.cis1_info_buf[..9].copy_from_slice(b"abc\0defg\0");
        cis.cis1_info[0] = Some(Cis1Info::Buf(0));
        cis.cis1_info[1] = Some(Cis1Info::Buf(4));
        cis.cis1_info[2] = Some(Cis1Info::Static(b"SDW-820"));
        assert_eq!(cis.info(0), Some(&b"abc"[..]));
        assert_eq!(cis.info(1), Some(&b"defg"[..]));
        assert_eq!(cis.info(2), Some(&b"SDW-820"[..]));
        assert_eq!(cis.info(3), None);
        assert_eq!(cis.info(4), None);
    }

    #[test]
    fn task_macros() {
        fn f(_: *mut c_void) {}
        let t = SdmmcTask::new();
        assert!(!sdmmc_task_pending(&t));
        sdmmc_init_task(&t, f, ptr::null_mut());
        assert!(t.func.get().is_some() && !sdmmc_task_pending(&t));
        assert_eq!(scf_cmd(SCF_CMD_ADTC | SCF_CMD_READ | SCF_RSP_R1), 0x50);
    }

    #[test]
    fn ioctl_numbers_are_the_lp64_ones() {
        // _IOWR('S', 0, 128 bytes) and _IOWR('S', 2, 16 bytes).
        assert_eq!(SDIOCEXECMMC, 0xc080_5300);
        assert_eq!(SDIOCEXECAPP, 0xc080_5301);
        assert_eq!(SDIOCSETDEBUG, 0xc010_5302);
    }

    #[test]
    #[ignore = "reads the C reference (just test-ref)"]
    fn defines_match_the_reference() {
        let defs = reftest::defines("sys/dev/sdmmc/sdmmcvar.h");
        #[allow(clippy::unnecessary_cast)] // a column of mixed types
        let ours: &[(&str, i64)] = &[
            ("SCF_ITSDONE", SCF_ITSDONE as i64),
            ("SCF_CMD_AC", SCF_CMD_AC as i64),
            ("SCF_CMD_ADTC", SCF_CMD_ADTC as i64),
            ("SCF_CMD_BC", SCF_CMD_BC as i64),
            ("SCF_CMD_BCR", SCF_CMD_BCR as i64),
            ("SCF_CMD_READ", SCF_CMD_READ as i64),
            ("SCF_RSP_BSY", SCF_RSP_BSY as i64),
            ("SCF_RSP_136", SCF_RSP_136 as i64),
            ("SCF_RSP_CRC", SCF_RSP_CRC as i64),
            ("SCF_RSP_IDX", SCF_RSP_IDX as i64),
            ("SCF_RSP_PRESENT", SCF_RSP_PRESENT as i64),
            ("SCF_RSP_R0", SCF_RSP_R0 as i64),
            ("SCF_RSP_R1", SCF_RSP_R1 as i64),
            ("SCF_RSP_R1B", SCF_RSP_R1B as i64),
            ("SCF_RSP_R2", SCF_RSP_R2 as i64),
            ("SCF_RSP_R3", SCF_RSP_R3 as i64),
            ("SCF_RSP_R4", SCF_RSP_R4 as i64),
            ("SCF_RSP_R5", SCF_RSP_R5 as i64),
            ("SCF_RSP_R5B", SCF_RSP_R5B as i64),
            ("SCF_RSP_R6", SCF_RSP_R6 as i64),
            ("SCF_RSP_R7", SCF_RSP_R7 as i64),
            ("SDMMC_VENDOR_INVALID", SDMMC_VENDOR_INVALID as i64),
            ("SDMMC_PRODUCT_INVALID", SDMMC_PRODUCT_INVALID as i64),
            ("SDMMC_FUNCTION_INVALID", SDMMC_FUNCTION_INVALID as i64),
            ("SFF_ERROR", SFF_ERROR as i64),
            ("SFF_SDHC", SFF_SDHC as i64),
            ("SMF_SD_MODE", SMF_SD_MODE as i64),
            ("SMF_IO_MODE", SMF_IO_MODE as i64),
            ("SMF_MEM_MODE", SMF_MEM_MODE as i64),
            ("SMF_UHS_MODE", SMF_UHS_MODE as i64),
            ("SMF_CARD_PRESENT", SMF_CARD_PRESENT as i64),
            ("SMF_CARD_ATTACHED", SMF_CARD_ATTACHED as i64),
            ("SMF_STOP_AFTER_MULTIPLE", SMF_STOP_AFTER_MULTIPLE as i64),
            ("SMF_CONFIG_PENDING", SMF_CONFIG_PENDING as i64),
            ("SMC_CAPS_AUTO_STOP", SMC_CAPS_AUTO_STOP as i64),
            ("SMC_CAPS_4BIT_MODE", SMC_CAPS_4BIT_MODE as i64),
            ("SMC_CAPS_DMA", SMC_CAPS_DMA as i64),
            ("SMC_CAPS_SPI_MODE", SMC_CAPS_SPI_MODE as i64),
            ("SMC_CAPS_POLL_CARD_DET", SMC_CAPS_POLL_CARD_DET as i64),
            ("SMC_CAPS_SINGLE_ONLY", SMC_CAPS_SINGLE_ONLY as i64),
            ("SMC_CAPS_8BIT_MODE", SMC_CAPS_8BIT_MODE as i64),
            ("SMC_CAPS_MULTI_SEG_DMA", SMC_CAPS_MULTI_SEG_DMA as i64),
            ("SMC_CAPS_SD_HIGHSPEED", SMC_CAPS_SD_HIGHSPEED as i64),
            ("SMC_CAPS_MMC_HIGHSPEED", SMC_CAPS_MMC_HIGHSPEED as i64),
            ("SMC_CAPS_UHS_SDR50", SMC_CAPS_UHS_SDR50 as i64),
            ("SMC_CAPS_UHS_SDR104", SMC_CAPS_UHS_SDR104 as i64),
            ("SMC_CAPS_UHS_DDR50", SMC_CAPS_UHS_DDR50 as i64),
            ("SMC_CAPS_UHS_MASK", SMC_CAPS_UHS_MASK as i64),
            ("SMC_CAPS_MMC_DDR52", SMC_CAPS_MMC_DDR52 as i64),
            ("SMC_CAPS_MMC_HS200", SMC_CAPS_MMC_HS200 as i64),
            ("SMC_CAPS_MMC_HS400", SMC_CAPS_MMC_HS400 as i64),
            ("SMC_CAPS_NONREMOVABLE", SMC_CAPS_NONREMOVABLE as i64),
        ];
        for &(name, v) in ours {
            assert_eq!(reftest::int(&defs, name), Some(v), "{name}");
        }
    }
}
/* </TESTS> */
