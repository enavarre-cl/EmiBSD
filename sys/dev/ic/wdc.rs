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
 * Copyright (c) 1998, 2001 Manuel Bouyer.  All rights reserved.
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
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
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
//! wdc(4): the IDE channel core every ATA/ATAPI controller front-end (`pciide`, the ISA and
//! PCMCIA `wdc` glue) drives its channels through. It probes a channel for drives (ATA by
//! poking the task file, ATAPI by its signature), attaches `wd(4)` or `atapiscsi(4)` to each,
//! runs the channel's queue of transfers (`struct wdc_xfer`) one at a time with their start,
//! interrupt and kill hooks, waits on the status register, resets the channel, picks the
//! drives' PIO/DMA/Ultra-DMA modes from their IDENTIFY data, runs the short commands of the
//! drive drivers (`wdc_exec_command`) and the `ATAIOCCOMMAND` pass-through.
//!
//! Upstream: sys/dev/ic/wdc.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `option WDCDEBUG` is the feature `wdcdebug`: `WDCDEBUG_PRINT` is [`wdcdebug_print!`],
//!   whose arguments are type-checked in a closure that is never called without it (as
//!   `ipsec_dprintf!`), and `wdc_log`, `wdc_get_log` and `ATAIOGETTRACE` are compiled only
//!   with it, as in the C. `WDCNDELAY_DEBUG` (`#if 0`) is a comment.
//! - `int` results that are booleans are `bool` (`wdc_floating_bus`, `wdc_preata_drive`,
//!   `wdc_ata_present`, `wdcreset`, `wdc_downgrade_mode`); `wdc_wait_for_status` returns
//!   `Err(ETIMEDOUT)` for -1, `wdc_dmawait` `Err(ENXIO)` for the C's -1 (controller gone)
//!   and `Err(ETIMEDOUT)` for its 1 (halted); `wdcdetach` and `wdc_ioctl` return `Result`.
//!   `wdcprobe` returns its drive mask and `wdc_exec_command` its `WDC_*` code, as the C.
//! - `wdc_exec_command` is an `unsafe fn`: the transfer keeps a pointer to the caller's
//!   `struct wdc_command` until it completes, which only `AT_WAIT` or `AT_POLL` guarantee
//!   before it returns.
//! - `wdcattach` takes the channel as `&'static`: the attach arguments of its drives point
//!   into it for as long as the front-end is attached.
//! - The default register functions transfer an odd byte count's last word through a
//!   two-byte bounce (`wdc_input_bytes` rounds a length up to a whole word, as the C, which
//!   reads or writes the byte past the buffer).
//! - `wdc_scrub_xfer` takes the transfer by pointer (it zeroes it whole) and is `unsafe`.
//! - The `xfer->c_start`-style calls through NULL, `chp->wdc` NULL and the missing hooks
//!   panic (the methods of `WdcXfer` and `ChannelSoftc`).
//! - `dma_alloc(9)` is not ported: `wdc_ioc_ata_cmd`'s buffer is `scsiconf`'s `DmaBuf`
//!   (`malloc(9)`).
//! - The xfer pool's `inited` flag is an atomic; the transfers come from `wdc_xfer_pool`
//!   through `wdc_xfer_iopool` as in the C.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};
#[cfg(feature = "wdcdebug")]
use core::sync::atomic::{AtomicI32, AtomicPtr, AtomicU8, AtomicU32};

use crate::dev::ata::ata::{ata_get_params, ata_set_mode};
use crate::dev::ata::atareg::{Ataparams, WDC_EXT_MODES, WDC_EXT_UDMA_MODES};
use crate::dev::ata::atavar::{
    AT_DF, AT_DONE, AT_ERROR, AT_POLL, AT_READ, AT_READREG, AT_TIMEOU, AT_WAIT, AT_WRITE,
    ATA_CONFIG_DMA_DISABLE, ATA_CONFIG_DMA_MODES, ATA_CONFIG_DMA_OFF, ATA_CONFIG_DMA_SET,
    ATA_CONFIG_PIO_MODES, ATA_CONFIG_PIO_OFF, ATA_CONFIG_PIO_SET, ATA_CONFIG_UDMA_DISABLE,
    ATA_CONFIG_UDMA_MODES, ATA_CONFIG_UDMA_OFF, ATA_CONFIG_UDMA_SET, AtaAtapiAttach, AtaDriveDatas,
    CMD_OK, DRIVE, DRIVE_ATA, DRIVE_ATAPI, DRIVE_CAP32, DRIVE_DMA, DRIVE_DMAERR, DRIVE_MODE,
    DRIVE_OLD, DRIVE_RESET, DRIVE_UDMA, T_ATA, T_ATAPI, WDC_COMPLETE, WDC_QUEUED, WDC_TRY_AGAIN,
    WdcCommand,
};
#[cfg(feature = "wdcdebug")]
use crate::dev::ic::wdcevent::WdceventType;
use crate::dev::ic::wdcevent::{
    WDC_LOG_ATA_CMDEXT, WDC_LOG_ATA_CMDLONG, WDC_LOG_ATA_CMDSHORT, WDC_LOG_ERROR, WDC_LOG_REG,
    WDC_LOG_SET_DRIVE, WDC_LOG_STATUS,
};
use crate::dev::ic::wdcreg::{
    ATAPI_IDENTIFY_DEVICE, ATAPI_SOFT_RESET, WDCC_CHECK_PWR, WDCC_IDENTIFY, WDCC_RECAL, WDCS_BITS,
    WDCS_BSY, WDCS_DRDY, WDCS_DRQ, WDCS_DSC, WDCS_DWF, WDCS_ERR, WDCTL_4BIT, WDCTL_IDS, WDCTL_RST,
    WDSD_IBM, WDSD_LBA,
};
use crate::dev::ic::wdcvar::{
    _WDC_AUX, C_ATAPI, C_POLL, C_PRIVATEXFER, C_SCSIXFER, C_TIMEOU, ChannelQueue, ChannelSoftc,
    ChannelSoftcVtbl, NOWAIT, VERBOSE, WDC_CAPABILITY_DATA16, WDC_CAPABILITY_DATA32,
    WDC_CAPABILITY_DMA, WDC_CAPABILITY_IRQACK, WDC_CAPABILITY_MODE, WDC_CAPABILITY_NO_ATAPI_DMA,
    WDC_CAPABILITY_NO_EXTRA_RESETS, WDC_CAPABILITY_PREATA, WDC_CAPABILITY_UDMA, WDC_DMAST_NOIRQ,
    WDC_NOSLEEP, WDC_QUIRK_NOATA, WDC_QUIRK_NOATAPI, WDC_RESET_WAIT, WDCF_ACTIVE, WDCF_DMA_WAIT,
    WDCF_IRQ_WAIT, WDCF_ONESLAVE, WdcRegs, WdcXfer, wait_for_drq, wait_for_unbusy, wdcwait,
    wdr_command, wdr_ctlr, wdr_cyl_hi, wdr_cyl_lo, wdr_error, wdr_features, wdr_lba_hi, wdr_lba_lo,
    wdr_lba_mi, wdr_sdh, wdr_seccnt, wdr_sector, wdr_status,
};
#[cfg(feature = "wdcdebug")]
use crate::dev::ic::wdcvar::{WDC_OPTION_PROBE_VERBOSE, WDCF_VERBOSE_PROBE};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::kern_synch::{tsleep_nsec, wakeup};
use crate::kern::kern_timeout::{timeout_add_msec, timeout_del, timeout_set};
use crate::kern::subr_autoconf::{config_detach_children, config_found};
use crate::kern::subr_pool::{pool_get, pool_init, pool_put};
use crate::kern::subr_prf::{Bitmask, Str, panic, printf};
use crate::machine::bus::{
    bus_space_read_1, bus_space_read_2, bus_space_read_4, bus_space_read_raw_multi_2,
    bus_space_read_raw_multi_4, bus_space_write_1, bus_space_write_2, bus_space_write_4,
    bus_space_write_raw_multi_2, bus_space_write_raw_multi_4,
};
use crate::machine::copy::{copyin, copyout};
use crate::machine::cpu::delay;
use crate::machine::intr::{IPL_BIO, splassert, splbio, splx};
use crate::scsi::scsi_base::{scsi_io_get, scsi_io_put, scsi_iopool_init};
use crate::scsi::scsiconf::{DmaBuf, SCSI_NOSLEEP, ScsiIo, ScsiIopool};
use crate::sys::ataio::{
    ATACMD_DF, ATACMD_ERROR, ATACMD_OK, ATACMD_READ, ATACMD_READREG, ATACMD_TIMEOUT, ATACMD_WRITE,
    ATAIOCCOMMAND, Atareq,
};
#[cfg(feature = "wdcdebug")]
use crate::sys::ataio::{ATAIOGETTRACE, Atagettrace};
use crate::sys::device::{Cfdriver, DV_DULL, UNCONF};
use crate::sys::errno::Errno::{self, *};
use crate::sys::fcntl::FWRITE;
use crate::sys::ioctl::{ioctl_arg, ioctl_ret};
#[cfg(feature = "wdcdebug")]
use crate::sys::malloc::M_TEMP;
use crate::sys::malloc::{M_DEVBUF, M_NOWAIT};
use crate::sys::param::{MAXPHYS, PRIBIO};
use crate::sys::pool::{PR_NOWAIT, PR_ZERO, Pool};
use crate::sys::proc::Proc;
use crate::sys::systm::{COLD, INFSLP};
use crate::sys::timeout::timeout_triggered;

/// `WDCDELAY`: 100 microseconds.
const WDCDELAY: i32 = 100;
/// `WDCNDELAY_RST`: the polls of a reset, `WDC_RESET_WAIT` ms in all.
const WDCNDELAY_RST: i32 = WDC_RESET_WAIT * 1000 / WDCDELAY;
// #if 0: WDCNDELAY_DEBUG 50 reports any delays more than WDCDELAY * N long.

/// `DEBUG_INTR`.
pub const DEBUG_INTR: i32 = 0x01;
/// `DEBUG_XFERS`.
pub const DEBUG_XFERS: i32 = 0x02;
/// `DEBUG_STATUS`.
pub const DEBUG_STATUS: i32 = 0x04;
/// `DEBUG_FUNCS`.
pub const DEBUG_FUNCS: i32 = 0x08;
/// `DEBUG_PROBE`.
pub const DEBUG_PROBE: i32 = 0x10;
/// `DEBUG_STATUSX`.
pub const DEBUG_STATUSX: i32 = 0x20;
/// `DEBUG_SDRIVE`.
pub const DEBUG_SDRIVE: i32 = 0x40;
/// `DEBUG_DETACH`.
pub const DEBUG_DETACH: i32 = 0x80;

/// `WDCDEBUG_MASK`: the messages printed at boot (`option WDCDEBUG`).
#[cfg(feature = "wdcdebug")]
const WDCDEBUG_MASK: i32 = 0x00;

/// `wdc_log_cap`: the trace ring's size.
#[cfg(feature = "wdcdebug")]
const WDC_LOG_CAP: u32 = 16 * 1024;

/// `WDCDEBUG_PRINT((fmt, args), level)`: prints when the kernel has `option WDCDEBUG`
/// (feature `wdcdebug`) and `level` is in the mask (`wdcdebug_mask` here and in `ata.c`,
/// `wdcdebug_wd_mask` in `ata_wdc.c` and `wd.c`). Without the feature the arguments are
/// type-checked inside a closure that is never called.
macro_rules! wdcdebug_print {
    ($mask:path, $level:expr, $($arg:tt)*) => {
        #[cfg(feature = "wdcdebug")]
        {
            if $mask.load(::core::sync::atomic::Ordering::Relaxed) & ($level) != 0 {
                let _ = $crate::kern::subr_prf::printf(::core::format_args!($($arg)*));
            }
        }
        #[cfg(not(feature = "wdcdebug"))]
        {
            let _ = || {
                let _ = $level;
                let _ = ::core::format_args!($($arg)*);
            };
        }
    };
}
pub(crate) use wdcdebug_print;

/// `scsi_iopool` made `Sync`: `wdc_xfer_iopool` is touched under its mutex and `splbio`.
struct WdcXferIopool(ScsiIopool);

// SAFETY: the pool's members change under its own mutex (`scsi_iopool_*`) or before any
// channel runs (`wdc_alloc_queue`), as in the C.
unsafe impl Sync for WdcXferIopool {}

/// `wdc_xfer_pool`.
static WDC_XFER_POOL: Pool = Pool::new();
/// `wdc_xfer_iopool`.
static WDC_XFER_IOPOOL: WdcXferIopool = WdcXferIopool(ScsiIopool::new());
/// `inited` of `wdc_alloc_queue`.
static WDC_QUEUE_INITED: AtomicBool = AtomicBool::new(false);

/// `wdcdebug_mask`.
#[cfg(feature = "wdcdebug")]
#[allow(non_upper_case_globals)] // the C name
pub static wdcdebug_mask: AtomicI32 = AtomicI32::new(WDCDEBUG_MASK);
/// `wdc_nxfer`.
#[cfg(feature = "wdcdebug")]
#[allow(non_upper_case_globals)] // the C name
pub static wdc_nxfer: AtomicI32 = AtomicI32::new(0);

/// `at_poll`: `AT_POLL` while the machine is cold, `AT_WAIT` once a channel attaches later.
#[allow(non_upper_case_globals)] // the C name
pub static at_poll: AtomicU16 = AtomicU16::new(AT_POLL);

/// `wdc_cd`.
pub static WDC_CD: Cfdriver = Cfdriver::new(b"wdc", DV_DULL, 0);

/// `wdc_default_vtbl`: the plain task file.
pub static WDC_DEFAULT_VTBL: ChannelSoftcVtbl = ChannelSoftcVtbl {
    read_reg: wdc_default_read_reg,
    write_reg: wdc_default_write_reg,
    lba48_write_reg: wdc_default_lba48_write_reg,
    read_raw_multi_2: wdc_default_read_raw_multi_2,
    write_raw_multi_2: wdc_default_write_raw_multi_2,
    read_raw_multi_4: wdc_default_read_raw_multi_4,
    write_raw_multi_4: wdc_default_write_raw_multi_4,
};

/// `wdc_log_buf`.
#[cfg(feature = "wdcdebug")]
static WDC_LOG_BUF: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
/// `wdc_tail`.
#[cfg(feature = "wdcdebug")]
static WDC_TAIL: AtomicU32 = AtomicU32::new(0);
/// `wdc_head`.
#[cfg(feature = "wdcdebug")]
static WDC_HEAD: AtomicU32 = AtomicU32::new(0);
/// `chp_idx`.
#[cfg(feature = "wdcdebug")]
static CHP_IDX: AtomicU8 = AtomicU8::new(1);

/// `wdc_log`: appends a record of `type` with payload `val` to the trace ring, dropping the
/// oldest records when it is full.
#[cfg(feature = "wdcdebug")]
pub fn wdc_log(chp: &ChannelSoftc, r#type: WdceventType, val: &[u8]) {
    let size = val.len() as u32;
    let mut head = WDC_HEAD.load(Ordering::Relaxed);
    let mut tail = WDC_TAIL.load(Ordering::Relaxed);
    let cap = WDC_LOG_CAP;

    #[cfg(feature = "diagnostic")]
    {
        if head > cap || tail > cap {
            let _ = printf(format_args!(
                "wdc_log: head {:x} wdc_tail {:x}\n",
                head, tail
            ));
            return;
        }

        if size > cap / 2 {
            let _ = printf(format_args!(
                "wdc_log: type {} size {:x}\n",
                r#type as u8, size
            ));
            return;
        }
    }

    let mut buf = WDC_LOG_BUF.load(Ordering::Relaxed);
    if buf.is_null() {
        let Some(p) = malloc(cap as usize, M_DEVBUF, M_NOWAIT) else {
            return;
        };
        buf = p.as_ptr();
        WDC_LOG_BUF.store(buf, Ordering::Relaxed);
    }
    if chp.ch_log_idx.get() == 0 {
        chp.ch_log_idx.set(CHP_IDX.fetch_add(1, Ordering::Relaxed));
    }

    // SAFETY: `buf` is the ring's `cap`-byte allocation, which lives forever; the ring is
    // only touched here and in `wdc_get_log` (under `splbio`).
    let log = unsafe { core::slice::from_raw_parts_mut(buf, cap as usize) };

    let request_size = size + 2;

    // Check how many bytes are left
    let mut log_size = head as i32 - tail as i32;
    if log_size < 0 {
        log_size += cap as i32;
    }

    if log_size as u32 + request_size >= cap {
        let mut nb = 0;

        while nb <= request_size * 2 {
            let rec_size = if log[tail as usize] == 0 {
                1
            } else {
                u32::from(log[tail as usize + 1] & 0x1f) + 2
            };
            tail = (tail + rec_size) % cap;
            nb += rec_size;
        }
    }

    // Avoid wrapping in the middle of a request
    if head + request_size >= cap {
        log[head as usize..].fill(0);
        head = 0;
    }

    let h = head as usize;
    log[h] = r#type as u8;
    log[h + 1] = ((chp.ch_log_idx.get() & 0x7) << 5) | (size & 0x1f) as u8;
    log[h + 2..h + 2 + val.len()].copy_from_slice(val);

    WDC_HEAD.store((head + request_size) % cap, Ordering::Relaxed);
    WDC_TAIL.store(tail, Ordering::Relaxed);
}

/// `wdc_get_log`: takes up to `*size` bytes of whole records off the ring into a new
/// `M_TEMP` buffer; `*size` becomes the bytes copied and `*left` what stays in the ring.
#[cfg(feature = "wdcdebug")]
pub fn wdc_get_log(size: &mut u32, left: &mut u32) -> Option<NonNull<u8>> {
    let head = WDC_HEAD.load(Ordering::Relaxed);
    let mut tail = WDC_TAIL.load(Ordering::Relaxed);
    let cap = WDC_LOG_CAP;
    let mut retbuf = None;

    let s = splbio();

    'out: {
        let mut log_size = head as i32 - tail as i32;
        *left = 0;

        if log_size < 0 {
            log_size += cap as i32;
        }

        let mut tocopy = log_size;
        if tocopy as u32 > *size {
            tocopy = *size as i32;
        }

        let buf = WDC_LOG_BUF.load(Ordering::Relaxed);
        if buf.is_null() {
            *size = 0;
            *left = 0;
            break 'out;
        }

        #[cfg(feature = "diagnostic")]
        if head > cap || tail > cap {
            let _ = printf(format_args!("wdc_log: head {:x} tail {:x}\n", head, tail));
            *size = 0;
            *left = 0;
            break 'out;
        }

        let Some(rb) = malloc(tocopy as usize, M_TEMP, M_NOWAIT) else {
            *size = 0;
            *left = log_size as u32;
            break 'out;
        };

        // SAFETY: the ring's `cap`-byte allocation (see `wdc_log`), read under `splbio`.
        let log = unsafe { core::slice::from_raw_parts(buf, cap as usize) };
        // SAFETY: `rb` is the `tocopy` bytes just allocated, ours alone.
        let out = unsafe { core::slice::from_raw_parts_mut(rb.as_ptr(), tocopy as usize) };

        let mut nb: i32 = 0;
        loop {
            let t = tail as usize;
            let rec_size = if log[t] == 0 {
                1
            } else {
                i32::from(log[t + 1] & 0x1f) + 2
            };

            if nb + rec_size >= tocopy {
                break;
            }

            out[nb as usize..(nb + rec_size) as usize]
                .copy_from_slice(&log[t..t + rec_size as usize]);
            tail = (tail + rec_size as u32) % cap;
            nb += rec_size;
        }

        WDC_TAIL.store(tail, Ordering::Relaxed);
        *size = nb as u32;
        *left = (log_size - nb) as u32;
        retbuf = Some(rb);
    }

    // out:
    splx(s);
    retbuf
}

/// `wdc_default_read_reg`.
pub fn wdc_default_read_reg(chp: &ChannelSoftc, reg: WdcRegs) -> u8 {
    #[cfg(feature = "diagnostic")]
    if reg.isset(crate::dev::ic::wdcvar::_WDC_WRONLY) {
        let _ = printf(format_args!(
            "wdc_default_read_reg: reading from a write-only register {}\n",
            reg.0
        ));
    }

    if reg.isset(_WDC_AUX) {
        bus_space_read_1(chp.ctl_iot(), chp.ctl_ioh(), reg.offset())
    } else {
        bus_space_read_1(chp.cmd_iot(), chp.cmd_ioh(), reg.offset())
    }
}

/// `wdc_default_write_reg`.
pub fn wdc_default_write_reg(chp: &ChannelSoftc, reg: WdcRegs, val: u8) {
    #[cfg(feature = "diagnostic")]
    if reg.isset(crate::dev::ic::wdcvar::_WDC_RDONLY) {
        let _ = printf(format_args!(
            "wdc_default_write_reg: writing to a read-only register {}\n",
            reg.0
        ));
    }

    if reg.isset(_WDC_AUX) {
        bus_space_write_1(chp.ctl_iot(), chp.ctl_ioh(), reg.offset(), val);
    } else {
        bus_space_write_1(chp.cmd_iot(), chp.cmd_ioh(), reg.offset(), val);
    }
}

/// `wdc_default_lba48_write_reg`: all registers are two byte deep FIFOs, high byte first.
pub fn wdc_default_lba48_write_reg(chp: &ChannelSoftc, reg: WdcRegs, val: u16) {
    chp.write_reg(reg, (val >> 8) as u8);
    chp.write_reg(reg, val as u8);
}

/// `wdc_default_read_raw_multi_2`: `nbytes` bytes from the data register, into `data` or
/// the bit bucket.
pub fn wdc_default_read_raw_multi_2(chp: &ChannelSoftc, data: Option<&mut [u8]>, nbytes: u32) {
    let (iot, ioh) = (chp.cmd_iot(), chp.cmd_ioh());
    let Some(data) = data else {
        for _ in (0..nbytes).step_by(2) {
            let _ = bus_space_read_2(iot, ioh, 0);
        }

        return;
    };

    let n = (nbytes as usize).min(data.len());
    let whole = n & !1;
    bus_space_read_raw_multi_2(iot, ioh, 0, &mut data[..whole]);
    // An odd tail: the last word through a bounce (see the module's deviations).
    for off in (whole..nbytes as usize).step_by(2) {
        let w = bus_space_read_2(iot, ioh, 0).to_ne_bytes();
        if off < n {
            let k = (n - off).min(2);
            data[off..off + k].copy_from_slice(&w[..k]);
        }
    }
}

/// `wdc_default_write_raw_multi_2`: `nbytes` bytes of `data`, or zeros, to the data
/// register.
pub fn wdc_default_write_raw_multi_2(chp: &ChannelSoftc, data: Option<&[u8]>, nbytes: u32) {
    let (iot, ioh) = (chp.cmd_iot(), chp.cmd_ioh());
    let Some(data) = data else {
        for _ in (0..nbytes).step_by(2) {
            bus_space_write_2(iot, ioh, 0, 0);
        }

        return;
    };

    let n = (nbytes as usize).min(data.len());
    let whole = n & !1;
    bus_space_write_raw_multi_2(iot, ioh, 0, &data[..whole]);
    for off in (whole..nbytes as usize).step_by(2) {
        let mut w = [0u8; 2];
        if off < n {
            let k = (n - off).min(2);
            w[..k].copy_from_slice(&data[off..off + k]);
        }
        bus_space_write_2(iot, ioh, 0, u16::from_ne_bytes(w));
    }
}

/// `wdc_default_write_raw_multi_4`.
pub fn wdc_default_write_raw_multi_4(chp: &ChannelSoftc, data: Option<&[u8]>, nbytes: u32) {
    let (iot, ioh) = (chp.cmd_iot(), chp.cmd_ioh());
    let Some(data) = data else {
        for _ in (0..nbytes).step_by(4) {
            bus_space_write_4(iot, ioh, 0, 0);
        }

        return;
    };

    let n = (nbytes as usize).min(data.len());
    let whole = n & !3;
    bus_space_write_raw_multi_4(iot, ioh, 0, &data[..whole]);
    for off in (whole..nbytes as usize).step_by(4) {
        let mut w = [0u8; 4];
        if off < n {
            let k = (n - off).min(4);
            w[..k].copy_from_slice(&data[off..off + k]);
        }
        bus_space_write_4(iot, ioh, 0, u32::from_ne_bytes(w));
    }
}

/// `wdc_default_read_raw_multi_4`.
pub fn wdc_default_read_raw_multi_4(chp: &ChannelSoftc, data: Option<&mut [u8]>, nbytes: u32) {
    let (iot, ioh) = (chp.cmd_iot(), chp.cmd_ioh());
    let Some(data) = data else {
        for _ in (0..nbytes).step_by(4) {
            let _ = bus_space_read_4(iot, ioh, 0);
        }

        return;
    };

    let n = (nbytes as usize).min(data.len());
    let whole = n & !3;
    bus_space_read_raw_multi_4(iot, ioh, 0, &mut data[..whole]);
    for off in (whole..nbytes as usize).step_by(4) {
        let w = bus_space_read_4(iot, ioh, 0).to_ne_bytes();
        if off < n {
            let k = (n - off).min(4);
            data[off..off + k].copy_from_slice(&w[..k]);
        }
    }
}

/// `wdprint`: the attach message of a drive found on a channel.
pub fn wdprint(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: `wdcattach` passes its `struct ata_atapi_attach` as `aux`.
    let aa_link = unsafe { &*aux.cast::<AtaAtapiAttach>().cast_const() };
    if let Some(pnp) = pnp {
        let _ = printf(format_args!("drive at {}", Str(pnp)));
    }
    let drive = aa_link.aa_drv_data.map_or(0, |d| d.drive.get());
    let _ = printf(format_args!(
        " channel {} drive {}",
        aa_link.aa_channel, drive
    ));
    UNCONF
}

/// `wdc_disable_intr`.
pub fn wdc_disable_intr(chp: &ChannelSoftc) {
    chp.write_reg(wdr_ctlr, WDCTL_IDS);
}

/// `wdc_enable_intr`.
pub fn wdc_enable_intr(chp: &ChannelSoftc) {
    chp.write_reg(wdr_ctlr, WDCTL_4BIT);
}

/// `wdc_set_drive`.
pub fn wdc_set_drive(chp: &ChannelSoftc, drive: i32) {
    chp.write_reg(wdr_sdh, ((drive << 4) as u8) | WDSD_IBM);
    WDC_LOG_SET_DRIVE(chp, drive as u8);
}

/// `chp->wdc->sc_dev.dv_xname`, or `"wdcprobe"` for a channel probed without a controller.
fn probe_name(chp: &ChannelSoftc) -> &str {
    chp.wdc_opt().map_or("wdcprobe", |w| w.sc_dev.xname())
}

/// `wdc_floating_bus`: the status register floats (no drive), per the Phoenix BIOS Drive
/// Autotyping document.
pub fn wdc_floating_bus(chp: &ChannelSoftc, drive: i32) -> bool {
    wdc_set_drive(chp, drive);
    delay(10);

    /* Stolen from Phoenix BIOS Drive Autotyping document */
    let mut cumulative_status = 0u8;
    for _ in 0..100 {
        chp.write_reg(wdr_seccnt, 0x7f);
        delay(1);

        let status = chp.read_reg(wdr_status);

        // The other bits are meaningless if BSY is set
        if status & WDCS_BSY != 0 {
            continue;
        }

        cumulative_status |= status;

        const BAD_BIT_COMBO: u8 = WDCS_DRDY | WDCS_DSC | WDCS_DRQ | WDCS_ERR;
        if cumulative_status & BAD_BIT_COMBO == BAD_BIT_COMBO {
            return true;
        }
    }

    false
}

/// `wdc_preata_drive`: a pre-ATA drive answers RECALIBRATE.
pub fn wdc_preata_drive(chp: &ChannelSoftc, drive: i32) -> bool {
    if wdc_floating_bus(chp, drive) {
        wdcdebug_print!(
            wdcdebug_mask,
            DEBUG_PROBE,
            "{}:{}:{}: floating bus detected\n",
            chp.wdc().sc_dev.xname(),
            chp.channel.get(),
            drive
        );
        return false;
    }

    wdc_set_drive(chp, drive);
    delay(100);
    if wdcwait(chp, WDCS_DRDY | WDCS_DRQ, WDCS_DRDY, 10000).is_err() {
        wdcdebug_print!(
            wdcdebug_mask,
            DEBUG_PROBE,
            "{}:{}:{}: not ready\n",
            chp.wdc().sc_dev.xname(),
            chp.channel.get(),
            drive
        );
        return false;
    }

    chp.write_reg(wdr_command, WDCC_RECAL);
    WDC_LOG_ATA_CMDSHORT(chp, WDCC_RECAL);
    if wdcwait(chp, WDCS_DRDY | WDCS_DRQ, WDCS_DRDY, 10000).is_err() {
        wdcdebug_print!(
            wdcdebug_mask,
            DEBUG_PROBE,
            "{}:{}:{}: WDCC_RECAL failed\n",
            chp.wdc().sc_dev.xname(),
            chp.channel.get(),
            drive
        );
        return false;
    }

    true
}

/// `wdc_ata_present`: an ATA drive is ready (DRDY and DSC) and its registers hold what is
/// written to them.
pub fn wdc_ata_present(chp: &ChannelSoftc, drive: i32) -> bool {
    let mut retry_cnt = 0;

    wdc_set_drive(chp, drive);
    delay(10);

    let time_to_done = loop {
        // You're actually supposed to wait up to 10 seconds for DRDY. However, as a
        // practical matter, most drives assert DRDY very quickly after dropping BSY.
        //
        // The 10 seconds wait is sub-optimal because, according to the ATA standard, the
        // master should reply with 00 for any reads to a non-existent slave.
        match wdc_wait_for_status(
            chp,
            WDCS_DRDY | WDCS_DSC | WDCS_DRQ,
            WDCS_DRDY | WDCS_DSC,
            1000,
        ) {
            Ok(t) => break t,
            Err(_) => {
                if retry_cnt == 0 && chp.ch_status.get() == 0x00 {
                    // At least one flash card needs to be kicked
                    wdccommandshort(chp, drive, WDCC_CHECK_PWR);
                    retry_cnt += 1;
                    continue; // goto retry
                }
                wdcdebug_print!(
                    wdcdebug_mask,
                    DEBUG_PROBE,
                    "{}:{}:{}: DRDY test timed out with status {:02x}\n",
                    probe_name(chp),
                    chp.channel.get(),
                    drive,
                    chp.ch_status.get()
                );
                return false;
            }
        }
    };

    if chp.ch_status.get() & 0xfc != WDCS_DRDY | WDCS_DSC {
        wdcdebug_print!(
            wdcdebug_mask,
            DEBUG_PROBE,
            "{}:{}:{}: status test for 0x50 failed with {:02x}\n",
            probe_name(chp),
            chp.channel.get(),
            drive,
            chp.ch_status.get()
        );

        return false;
    }

    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_PROBE,
        "{}:{}:{}: waiting for ready {} msec\n",
        probe_name(chp),
        chp.channel.get(),
        drive,
        time_to_done
    );

    // Test register writability
    chp.write_reg(wdr_cyl_lo, 0xaa);
    chp.write_reg(wdr_cyl_hi, 0x55);
    chp.write_reg(wdr_seccnt, 0xff);
    delay(10);

    if chp.read_reg(wdr_cyl_lo) != 0xaa && chp.read_reg(wdr_cyl_hi) != 0x55 {
        wdcdebug_print!(
            wdcdebug_mask,
            DEBUG_PROBE,
            "{}:{}:{}: register writability failed\n",
            probe_name(chp),
            chp.channel.get(),
            drive
        );
        return false;
    }

    true
}

/// `wdcprobe`: tests to see controller with at least one attached drive is there. Returns a
/// bit for each possible drive found (0x01 for drive 0, 0x02 for drive 1).
///
/// Logic:
/// - If a status register is at 0x7f or 0xff, assume there is no drive here (ISA has pull-up
///   resistors). Similarly if the status register has the value we last wrote to the bus
///   (for IDE interfaces without pullups). If no drive at all -> return.
/// - reset the controller, wait for it to complete (may take up to 31s !). If timeout ->
///   return.
/// - test ATA/ATAPI signatures. If at last one drive found -> return.
/// - try an ATA command on the master.
pub fn wdcprobe(chp: &ChannelSoftc) -> i32 {
    let mut ret_value: i32 = 0x03;
    #[cfg(feature = "wdcdebug")]
    let savedmask = wdcdebug_mask.load(Ordering::Relaxed);

    if chp._vtbl.get().is_none() {
        let s = splbio();
        chp._vtbl.set(Some(&WDC_DEFAULT_VTBL));
        splx(s);
    }

    #[cfg(feature = "wdcdebug")]
    if chp.isset(WDCF_VERBOSE_PROBE)
        || chp
            .wdc_opt()
            .is_some_and(|w| w.sc_dev.cfdata().cf_flags & WDC_OPTION_PROBE_VERBOSE != 0)
    {
        wdcdebug_mask.fetch_or(DEBUG_PROBE, Ordering::Relaxed);
    }

    if chp
        .wdc_opt()
        .is_none_or(|w| !w.has(WDC_CAPABILITY_NO_EXTRA_RESETS))
    {
        // Sample the statuses of drive 0 and 1 into st0 and st1
        wdc_set_drive(chp, 0);
        delay(10);
        let st0 = chp.read_reg(wdr_status);
        WDC_LOG_STATUS(chp, st0);
        wdc_set_drive(chp, 1);
        delay(10);
        let st1 = chp.read_reg(wdr_status);
        WDC_LOG_STATUS(chp, st1);

        wdcdebug_print!(
            wdcdebug_mask,
            DEBUG_PROBE,
            "{}:{}: before reset, st0=0x{}, st1=0x{}\n",
            probe_name(chp),
            chp.channel.get(),
            Bitmask(u64::from(st0), WDCS_BITS),
            Bitmask(u64::from(st1), WDCS_BITS)
        );

        if st0 == 0xff || st0 == WDSD_IBM {
            ret_value &= !0x01;
        }
        if st1 == 0xff || st1 == (WDSD_IBM | 0x10) {
            ret_value &= !0x02;
        }
        if ret_value == 0 {
            return 0;
        }
    }

    // reset the channel
    wdc_do_reset(chp);

    ret_value = __wdcwait_reset(chp, ret_value);
    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_PROBE,
        "{}:{}: after reset, ret_value=0x{}\n",
        probe_name(chp),
        chp.channel.get(),
        ret_value
    );

    if ret_value == 0 {
        return 0;
    }

    if !chp
        .wdc_opt()
        .is_some_and(|w| w.quirks.get() & WDC_QUIRK_NOATAPI != 0)
    {
        // Use signatures to find potential ATAPI drives
        for drive in 0..2 {
            if ret_value & (0x01 << drive) == 0 {
                continue;
            }
            wdc_set_drive(chp, drive);
            delay(10);
            // Save registers contents
            let st0 = chp.read_reg(wdr_status);
            let sc = chp.read_reg(wdr_seccnt);
            let sn = chp.read_reg(wdr_sector);
            let cl = chp.read_reg(wdr_cyl_lo);
            let ch = chp.read_reg(wdr_cyl_hi);
            WDC_LOG_REG(chp, wdr_cyl_lo, (u16::from(ch) << 8) | u16::from(cl));

            wdcdebug_print!(
                wdcdebug_mask,
                DEBUG_PROBE,
                "{}:{}:{}: after reset, st=0x{}, sc=0x{:x} sn=0x{:x} cl=0x{:x} ch=0x{:x}\n",
                probe_name(chp),
                chp.channel.get(),
                drive,
                Bitmask(u64::from(st0), WDCS_BITS),
                sc,
                sn,
                cl,
                ch
            );
            // This is a simplification of the test in the ATAPI spec since not all drives
            // seem to set the other regs correctly.
            if cl == 0x14 && ch == 0xeb {
                chp.ch_drive[drive as usize].set(DRIVE_ATAPI);
            }
        }
    }

    // noatapi:
    if !chp
        .wdc_opt()
        .is_some_and(|w| w.quirks.get() & WDC_QUIRK_NOATA != 0)
    {
        // Detect ATA drives by poking around the registers
        for drive in 0..2 {
            if ret_value & (0x01 << drive) == 0 {
                continue;
            }
            let drvp = &chp.ch_drive[drive as usize];
            if drvp.isset(DRIVE_ATAPI) {
                continue;
            }

            wdc_disable_intr(chp);
            // ATA detect
            if wdc_ata_present(chp, drive) {
                drvp.set(DRIVE_ATA);
                if chp.wdc_opt().is_none_or(|w| w.has(WDC_CAPABILITY_PREATA)) {
                    drvp.set(DRIVE_OLD);
                }
            } else {
                ret_value &= !(1 << drive);
            }
            wdc_enable_intr(chp);
        }
    }

    // noata:
    #[cfg(feature = "wdcdebug")]
    wdcdebug_mask.store(savedmask, Ordering::Relaxed);
    ret_value
}

/// `wdc_alloc_queue`: a channel queue, setting up the transfer pool on first use.
pub fn wdc_alloc_queue() -> Option<NonNull<ChannelQueue>> {
    // Initialize global data.
    if !WDC_QUEUE_INITED.load(Ordering::Relaxed) {
        // Initialize the wdc_xfer pool.
        pool_init(
            &WDC_XFER_POOL,
            size_of::<WdcXfer>(),
            0,
            IPL_BIO,
            0,
            "wdcxfer",
            None,
        );
        // SAFETY: `wdc_xfer_get` and `wdc_xfer_put` ignore the cookie, and work on the
        // transfers of `WDC_XFER_POOL`, which `wdc_xfer_get` hands out.
        unsafe {
            scsi_iopool_init(
                &WDC_XFER_IOPOOL.0,
                ptr::null_mut(),
                wdc_xfer_get,
                wdc_xfer_put,
            )
        };
        WDC_QUEUE_INITED.store(true, Ordering::Relaxed);
    }

    let queue = malloc(size_of::<ChannelQueue>(), M_DEVBUF, M_NOWAIT)?.cast::<ChannelQueue>();
    // SAFETY: `queue` is a fresh allocation the size of a `ChannelQueue` (malloc(9) aligns
    // to the largest type), written whole before the reference is made; the queue head is
    // initialised in place, where it stays.
    unsafe {
        queue.as_ptr().write(ChannelQueue::new());
        queue.as_ref().sc_xfer.init();
    }
    Some(queue)
}

/// `wdc_free_queue`.
///
/// # Safety
///
/// `queue` came from [`wdc_alloc_queue`], and no channel uses it any more.
pub unsafe fn wdc_free_queue(queue: NonNull<ChannelQueue>) {
    free(queue.cast(), M_DEVBUF, size_of::<ChannelQueue>());
}

/// `wdcattach`: finds the drives of a channel the front-end set up, reads their
/// IDENTIFY data and attaches a driver to each.
pub fn wdcattach(chp: &'static ChannelSoftc) {
    #[cfg(feature = "wdcdebug")]
    let savedmask = wdcdebug_mask.load(Ordering::Relaxed);

    if !COLD.load(Ordering::Relaxed) {
        at_poll.store(AT_WAIT, Ordering::Relaxed);
    }

    let wdc = chp.wdc();
    if wdc.reset.get().is_none() {
        wdc.reset.set(Some(wdc_do_reset));
    }

    timeout_set(
        &chp.ch_timo,
        wdctimeout,
        ptr::from_ref(chp).cast_mut().cast(),
    );

    if chp._vtbl.get().is_none() {
        chp._vtbl.set(Some(&WDC_DEFAULT_VTBL));
    }

    for (i, drvp) in chp.ch_drive.iter().enumerate() {
        drvp.chnl_softc.set(ptr::from_ref(chp).cast_mut().cast());
        drvp.drive.set(i as u8);
    }

    if let Some(drv_probe) = wdc.drv_probe.get() {
        drv_probe(chp);
    } else if wdcprobe(chp) == 0 {
        // If no drives, abort attach here.
        return;
    }

    // ATAPI drives need settling time. Give them 250ms
    if chp.ch_drive[0].isset(DRIVE_ATAPI) || chp.ch_drive[1].isset(DRIVE_ATAPI) {
        delay(250 * 1000);
    }

    #[cfg(feature = "wdcdebug")]
    {
        if wdc.sc_dev.cfdata().cf_flags & WDC_OPTION_PROBE_VERBOSE != 0 {
            wdcdebug_mask.fetch_or(DEBUG_PROBE, Ordering::Relaxed);
        }

        if chp.ch_drive[0].isset(DRIVE_ATAPI) || chp.ch_drive[1].isset(DRIVE_ATAPI) {
            wdcdebug_mask.store(DEBUG_PROBE, Ordering::Relaxed);
        }
    }

    for i in 0..2 {
        let drvp = &chp.ch_drive[i];

        // If controller can't do 16bit flag the drives as 32bit
        if wdc.cap.get() & (WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32) == WDC_CAPABILITY_DATA32
        {
            drvp.set(DRIVE_CAP32);
        }

        if !drvp.isset(DRIVE) {
            continue;
        }

        if i == 1 && !chp.ch_drive[0].isset(DRIVE) {
            chp.set(WDCF_ONESLAVE);
        }
        // Wait a bit, some devices are weird just after a reset. Then issue a IDENTIFY
        // command, to try to detect slave ghost.
        delay(5000);
        // SAFETY: the channel's attach: nothing else reads the drive's IDENTIFY block
        // before its driver attaches below.
        let id = unsafe { drvp.id_mut() };
        if ata_get_params(drvp, at_poll.load(Ordering::Relaxed) as u8, id) == CMD_OK {
            // If IDENTIFY succeeded, this is not an OLD ctrl
            drvp.clr(DRIVE_OLD);
        } else {
            *id = Ataparams::zeroed();
            drvp.clr(DRIVE_ATA | DRIVE_ATAPI);
            wdcdebug_print!(
                wdcdebug_mask,
                DEBUG_PROBE,
                "{}:{}:{}: IDENTIFY failed\n",
                wdc.sc_dev.xname(),
                chp.channel.get(),
                i
            );

            if drvp.isset(DRIVE_OLD) && !wdc_preata_drive(chp, i as i32) {
                drvp.clr(DRIVE_OLD);
            }
        }
    }

    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_PROBE,
        "wdcattach: ch_drive_flags 0x{:x} 0x{:x}\n",
        chp.ch_drive[0].drive_flags.get(),
        chp.ch_drive[1].drive_flags.get()
    );

    // If no drives, abort here
    if chp.ch_drive[0].isset(DRIVE) || chp.ch_drive[1].isset(DRIVE) {
        for drvp in &chp.ch_drive {
            if !drvp.isset(DRIVE) {
                continue;
            }
            let mut aa_link = AtaAtapiAttach {
                aa_type: if drvp.isset(DRIVE_ATAPI) {
                    T_ATAPI
                } else {
                    T_ATA
                },
                aa_channel: chp.channel.get() as u8,
                aa_openings: 1,
                aa_drv_data: Some(drvp),
                aa_bus_private: ptr::null_mut(),
            };
            let _ = config_found(
                &wdc.sc_dev,
                ptr::from_mut(&mut aa_link).cast(),
                Some(wdprint),
            );
        }

        // reset drive_flags for unattached devices, reset state for attached ones
        for drvp in &chp.ch_drive {
            if drvp.drive_name.get()[0] == 0 {
                drvp.drive_flags.set(0);
            }
        }
    }

    // exit:
    #[cfg(feature = "wdcdebug")]
    wdcdebug_mask.store(savedmask, Ordering::Relaxed);
}

/// `wdcstart`: starts I/O on a controller, for the given channel. The first xfer may be not
/// for our channel if the channel queues are shared.
pub fn wdcstart(chp: &ChannelSoftc) {
    splassert(IPL_BIO, "wdcstart");

    // is there a xfer ?
    let Some(xfer) = chp.queue().sc_xfer.first() else {
        return;
    };

    // adjust chp, in case we have a shared queue
    let chp = xfer.chp();

    if chp.isset(WDCF_ACTIVE) {
        return; // channel already active
    }
    #[cfg(feature = "diagnostic")]
    if chp.isset(WDCF_IRQ_WAIT) {
        panic(format_args!("wdcstart: channel waiting for irq"));
    }

    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_XFERS,
        "wdcstart: xfer {:p} channel {} drive {}\n",
        xfer,
        chp.channel.get(),
        xfer.drive.get()
    );
    chp.set(WDCF_ACTIVE);
    let drvp = &chp.ch_drive[usize::from(xfer.drive.get())];
    if drvp.isset(DRIVE_RESET) {
        drvp.clr(DRIVE_RESET);
        drvp.state.set(0);
    }
    xfer.start(chp);
}

/// `wdcdetach`: kills the channel's pending transfers and detaches its drives.
pub fn wdcdetach(chp: &ChannelSoftc, flags: i32) -> Result<(), Errno> {
    let s = splbio();
    chp.dying.set(1);

    wdc_kill_pending(chp);
    let _ = timeout_del(&chp.ch_timo);

    let rv = config_detach_children(&chp.wdc().sc_dev, flags);
    splx(s);

    rv
}

/// `wdcintr`: interrupt routine for the controller. Acknowledge the interrupt, check for
/// errors on the current operation, mark it done if necessary, and start the next request.
/// Also check for a partially done transfer, and continue with the next chunk if so.
pub fn wdcintr(arg: *mut c_void) -> i32 {
    // SAFETY: the front-end establishes the interrupt with the channel softc, which lives
    // while the interrupt is established.
    let chp = unsafe { &*arg.cast::<ChannelSoftc>().cast_const() };

    if !chp.isset(WDCF_IRQ_WAIT) {
        // Acknowledge interrupt by reading status
        let st = if chp._vtbl.get().is_none() {
            bus_space_read_1(chp.cmd_iot(), chp.cmd_ioh(), wdr_status.offset())
        } else {
            chp.read_reg(wdr_status)
        };
        if st == 0xff {
            return -1;
        }

        wdcdebug_print!(wdcdebug_mask, DEBUG_INTR, "wdcintr: inactive controller\n");
        return 0;
    }

    wdcdebug_print!(wdcdebug_mask, DEBUG_INTR, "wdcintr\n");
    let Some(xfer) = chp.queue().sc_xfer.first() else {
        panic(format_args!(
            "wdcintr: channel {} waits for an irq with no xfer",
            chp.channel.get()
        ));
    };
    if chp.isset(WDCF_DMA_WAIT) {
        let wdc = chp.wdc();
        wdc.dma_status
            .set(wdc.dma_finish(chp.channel.get(), i32::from(xfer.drive.get()), 0));
        if wdc.dma_status.get() == 0xff {
            return -1;
        }
        if wdc.dma_status.get() & WDC_DMAST_NOIRQ != 0 {
            // IRQ not for us, not detected by DMA engine
            return 0;
        }
        chp.clr(WDCF_DMA_WAIT);
    }

    chp.clr(WDCF_IRQ_WAIT);
    let ret = xfer.intr(chp, 1);
    if ret == 0 {
        // irq was not for us, still waiting for irq
        chp.set(WDCF_IRQ_WAIT);
    }
    ret
}

/// The channel of a drive (`drvp->chnl_softc`), which `wdcattach` set.
pub fn wdc_drvp_chp(drvp: &AtaDriveDatas) -> &ChannelSoftc {
    let p = drvp.chnl_softc.get();
    if p.is_null() {
        panic(format_args!(
            "wdc: drive {} has no channel",
            drvp.drive.get()
        ));
    }
    // SAFETY: `wdcattach` points `chnl_softc` at the channel whose `ch_drive` holds this
    // drive's data, so the channel lives at least as long as `drvp`.
    unsafe { &*p.cast::<ChannelSoftc>().cast_const() }
}

/// `wdc_reset_channel`: puts all disks in RESET state.
pub fn wdc_reset_channel(drvp: &AtaDriveDatas, nowait: bool) {
    let chp = wdc_drvp_chp(drvp);

    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_FUNCS,
        "ata_reset_channel {}:{} for drive {}\n",
        chp.wdc().sc_dev.xname(),
        chp.channel.get(),
        drvp.drive.get()
    );
    let _ = wdcreset(chp, if nowait { NOWAIT } else { VERBOSE });
    for d in &chp.ch_drive {
        d.state.set(0);
    }
}

/// `wdcreset`: resets the channel through the controller's `reset` hook and, unless
/// `NOWAIT`, waits for the drives; true when a drive did not come back.
pub fn wdcreset(chp: &ChannelSoftc, flags: i32) -> bool {
    if chp._vtbl.get().is_none() {
        chp._vtbl.set(Some(&WDC_DEFAULT_VTBL));
    }

    let wdc = chp.wdc();
    match wdc.reset.get() {
        Some(reset) => reset(chp),
        None => panic(format_args!("{}: no reset", wdc.sc_dev.xname())),
    }

    if flags & NOWAIT != 0 {
        return false;
    }

    let mut drv_mask1 = if chp.ch_drive[0].isset(DRIVE) {
        0x01
    } else {
        0x00
    };
    drv_mask1 |= if chp.ch_drive[1].isset(DRIVE) {
        0x02
    } else {
        0x00
    };
    let drv_mask2 = __wdcwait_reset(chp, drv_mask1);

    if flags & VERBOSE != 0 && drv_mask2 != drv_mask1 {
        let _ = printf(format_args!(
            "{} channel {}: reset failed for",
            wdc.sc_dev.xname(),
            chp.channel.get()
        ));
        if drv_mask1 & 0x01 != 0 && drv_mask2 & 0x01 == 0 {
            let _ = printf(format_args!(" drive 0"));
        }
        if drv_mask1 & 0x02 != 0 && drv_mask2 & 0x02 == 0 {
            let _ = printf(format_args!(" drive 1"));
        }
        let _ = printf(format_args!("\n"));
    }

    drv_mask1 != drv_mask2
}

/// `wdc_do_reset`: the software reset through the control register.
pub fn wdc_do_reset(chp: &ChannelSoftc) {
    wdc_set_drive(chp, 0);
    delay(10);
    chp.write_reg(wdr_ctlr, WDCTL_4BIT | WDCTL_RST);
    delay(10000);
    chp.write_reg(wdr_ctlr, WDCTL_4BIT);
    delay(10000);
}

/// `__wdcwait_reset`: waits for the drives of `drv_mask` to drop BSY after a reset; returns
/// the drives that did.
pub fn __wdcwait_reset(chp: &ChannelSoftc, drv_mask: i32) -> i32 {
    let mut drv_mask = drv_mask;
    let (mut st0, mut er0, mut st1, mut er1) = (0u8, 0u8, 0u8, 0u8);
    let mut timeout = 0;

    // wait for BSY to deassert
    'end: {
        while timeout < WDCNDELAY_RST {
            wdc_set_drive(chp, 0);
            delay(10);
            st0 = chp.read_reg(wdr_status);
            er0 = chp.read_reg(wdr_error);
            wdc_set_drive(chp, 1);
            delay(10);
            st1 = chp.read_reg(wdr_status);
            er1 = chp.read_reg(wdr_error);

            if drv_mask & 0x01 == 0 {
                // no master
                if drv_mask & 0x02 != 0 && st1 & WDCS_BSY == 0 {
                    // No master, slave is ready, it's done
                    break 'end;
                }
            } else if drv_mask & 0x02 == 0 {
                // no slave
                if drv_mask & 0x01 != 0 && st0 & WDCS_BSY == 0 {
                    // No slave, master is ready, it's done
                    break 'end;
                }
            } else {
                // Wait for both master and slave to be ready
                if st0 & WDCS_BSY == 0 && st1 & WDCS_BSY == 0 {
                    break 'end;
                }
            }
            delay(WDCDELAY as u32);
            timeout += 1;
        }
        // Reset timed out. Maybe it's because drv_mask was not right
        if st0 & WDCS_BSY != 0 {
            drv_mask &= !0x01;
        }
        if st1 & WDCS_BSY != 0 {
            drv_mask &= !0x02;
        }
    }
    // end:
    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_PROBE,
        "{}:{}: wdcwait_reset() end, st0=0x{}, er0=0x{:x}, st1=0x{}, er1=0x{:x}, reset time={} msec\n",
        probe_name(chp),
        chp.channel.get(),
        Bitmask(u64::from(st0), WDCS_BITS),
        er0,
        Bitmask(u64::from(st1), WDCS_BITS),
        er1,
        timeout * WDCDELAY / 1000
    );

    drv_mask
}

/// `wdc_wait_for_status`: waits for a drive to be !BSY, and have `mask` in its status
/// register equal to `bits`. Returns the number of `WDCDELAY` polls it took, or
/// `ETIMEDOUT` after `timeout` ms.
pub fn wdc_wait_for_status(
    chp: &ChannelSoftc,
    mask: u8,
    bits: u8,
    timeout: i32,
) -> Result<i32, Errno> {
    let mut time = 0;

    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_STATUS,
        "wdcwait {}:{}\n",
        chp.wdc_opt().map_or("none", |w| w.sc_dev.xname()),
        chp.channel.get()
    );
    chp.ch_error.set(0);

    let timeout = timeout * 1000 / WDCDELAY; // delay uses microseconds

    let status = loop {
        let mut status = chp.read_reg(wdr_status);
        chp.ch_status.set(status);
        WDC_LOG_STATUS(chp, status);

        if status == 0xff && chp.isset(WDCF_ONESLAVE) {
            wdc_set_drive(chp, 1);
            status = chp.read_reg(wdr_status);
            chp.ch_status.set(status);
            WDC_LOG_STATUS(chp, status);
        }
        if status & WDCS_BSY == 0 && status & mask == bits {
            break status;
        }
        time += 1;
        if time > timeout {
            wdcdebug_print!(
                wdcdebug_mask,
                DEBUG_STATUSX | DEBUG_STATUS,
                "wdcwait: timeout, status 0x{} error 0x{:x}\n",
                Bitmask(u64::from(status), WDCS_BITS),
                chp.read_reg(wdr_error)
            );
            return Err(ETIMEDOUT);
        }
        delay(WDCDELAY as u32);
    };
    if status & WDCS_ERR != 0 {
        chp.ch_error.set(chp.read_reg(wdr_error));
        WDC_LOG_ERROR(chp, chp.ch_error.get());

        wdcdebug_print!(
            wdcdebug_mask,
            DEBUG_STATUSX | DEBUG_STATUS,
            "wdcwait: error {:x}\n",
            chp.ch_error.get()
        );
    }

    // #ifdef WDCNDELAY_DEBUG (#if 0): after autoconfig, warn of a busy-wait longer than
    // WDCNDELAY_DEBUG polls.
    Ok(time)
}

/// `wdc_dmawait`: busy-waits for DMA to complete; `Err(ENXIO)` when the controller went
/// away, `Err(ETIMEDOUT)` when the DMA had to be halted.
pub fn wdc_dmawait(chp: &ChannelSoftc, xfer: &WdcXfer, timeout: i32) -> Result<(), Errno> {
    let wdc = chp.wdc();
    let drive = i32::from(xfer.drive.get());
    for _ in 0..timeout * 1000 / WDCDELAY {
        wdc.dma_status
            .set(wdc.dma_finish(chp.channel.get(), drive, 0));
        if wdc.dma_status.get() & WDC_DMAST_NOIRQ == 0 {
            return Ok(());
        }
        if wdc.dma_status.get() == 0xff {
            chp.dying.set(1);
            return Err(ENXIO);
        }
        delay(WDCDELAY as u32);
    }
    // timeout, force a DMA halt
    wdc.dma_status
        .set(wdc.dma_finish(chp.channel.get(), drive, 1));
    Err(ETIMEDOUT)
}

/// `wdctimeout`: the channel's transfer timed out (or its interrupt was lost).
pub fn wdctimeout(arg: *mut c_void) {
    // SAFETY: `wdcattach` set the timeout with the channel, which outlives it (`wdcdetach`
    // deletes it).
    let chp = unsafe { &*arg.cast::<ChannelSoftc>().cast_const() };

    wdcdebug_print!(wdcdebug_mask, DEBUG_FUNCS, "wdctimeout\n");

    let s = splbio();
    let xfer = chp.queue().sc_xfer.first();

    // Did we lose a race with the interrupt?
    let Some(xfer) = xfer.filter(|_| timeout_triggered(&chp.ch_timo)) else {
        splx(s);
        return;
    };
    if chp.isset(WDCF_IRQ_WAIT) {
        __wdcerror(chp, "timeout");
        let _ = printf(format_args!(
            "\ttype: {}\n",
            if xfer.isset(C_ATAPI) { "atapi" } else { "ata" }
        ));
        let _ = printf(format_args!("\tc_bcount: {}\n", xfer.c_bcount.get()));
        let _ = printf(format_args!("\tc_skip: {}\n", xfer.c_skip.get()));
        if chp.isset(WDCF_DMA_WAIT) {
            let wdc = chp.wdc();
            wdc.dma_status
                .set(wdc.dma_finish(chp.channel.get(), i32::from(xfer.drive.get()), 1));
            chp.clr(WDCF_DMA_WAIT);
        }
        // Call the interrupt routine. If we just missed and interrupt, it will do what's
        // needed. Else, it will take the needed action (reset the device).
        xfer.set(C_TIMEOU);
        chp.clr(WDCF_IRQ_WAIT);
        let _ = xfer.intr(chp, 1);
    } else {
        __wdcerror(chp, "missing untimeout");
    }
    splx(s);
}

/// `wdc_probe_caps`: probes the drive's capabilities, for use by the controller later.
/// Assumes `drvp` points to an existing drive.
///
/// XXX this should be a controller-indep function
pub fn wdc_probe_caps(drvp: &AtaDriveDatas, params: &Ataparams) {
    let chp = wdc_drvp_chp(drvp);
    let wdc = chp.wdc();
    let cf_flags = drvp.cf_flags.get();
    let poll = at_poll.load(Ordering::Relaxed) as u8;

    if wdc.cap.get() & (WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32)
        == (WDC_CAPABILITY_DATA16 | WDC_CAPABILITY_DATA32)
    {
        let mut params2 = Ataparams::zeroed();

        // Controller claims 16 and 32 bit transfers. Re-do an IDENTIFY with 32-bit
        // transfers, and compare results.
        drvp.set(DRIVE_CAP32);
        let _ = ata_get_params(drvp, poll, &mut params2);
        if params.as_bytes() != params2.as_bytes() {
            // Not good. fall back to 16bits
            drvp.clr(DRIVE_CAP32);
        }
    }
    // #if 0: Some ultra-DMA drives claims to only support ATA-3. sigh (the ATA version
    // from atap_ata_major).
    // Use PIO mode 3 as a default value for ATAPI devices
    if drvp.isset(DRIVE_ATAPI) {
        drvp.PIO_mode.set(3);
    }

    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_PROBE,
        "wdc_probe_caps: wdc_cap 0x{:x} cf_flags 0x{:x}\n",
        wdc.cap.get(),
        cf_flags
    );

    let mut valid_mode_found = false;

    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_PROBE,
        "wdc_probe_caps: atap_oldpiotiming={}\n",
        params.atap_oldpiotiming
    );
    // ATA-4 compliant devices contain PIO mode number in atap_oldpiotiming.
    if params.atap_oldpiotiming <= 2 {
        drvp.PIO_cap.set(params.atap_oldpiotiming);
        valid_mode_found = true;
        drvp.set(DRIVE_MODE);
    } else if params.atap_oldpiotiming > 180 {
        // ATA-2 compliant devices contain cycle time in atap_oldpiotiming. A device with a
        // cycle time of 180ns or less is at least PIO mode 3 and should be reporting that
        // in atap_piomode_supp, so ignore it here.
        if params.atap_oldpiotiming <= 240 {
            drvp.PIO_cap.set(2);
        } else {
            drvp.PIO_cap.set(1);
        }
        valid_mode_found = true;
        drvp.set(DRIVE_MODE);
    }
    if valid_mode_found {
        drvp.PIO_mode.set(drvp.PIO_cap.get());
    }

    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_PROBE,
        "wdc_probe_caps: atap_extensions=0x{:x}, atap_piomode_supp=0x{:x}, atap_dmamode_supp=0x{:x}, atap_udmamode_supp=0x{:x}\n",
        params.atap_extensions,
        params.atap_piomode_supp,
        params.atap_dmamode_supp,
        params.atap_udmamode_supp
    );

    // It's not in the specs, but it seems that some drive returns 0xffff in
    // atap_extensions when this field is invalid
    if params.atap_extensions != 0xffff && params.atap_extensions & WDC_EXT_MODES != 0 {
        // XXX some drives report something wrong here (they claim to support PIO mode 8 !).
        // As mode is coded on 3 bits in SET FEATURE, limit it to 7 (so limit i to 4). If
        // higher mode than 7 is found, abort.
        for i in (0..=7u8).rev() {
            if params.atap_piomode_supp & (1 << i) == 0 {
                continue;
            }
            if i > 4 {
                return;
            }

            valid_mode_found = true;

            if !wdc.has(WDC_CAPABILITY_MODE) {
                drvp.PIO_cap.set(i + 3);
                continue;
            }

            // See if mode is accepted. If the controller can't set its PIO mode, assume
            // the BIOS set it up correctly
            if ata_set_mode(drvp, 0x08 | (i + 3), poll) != CMD_OK {
                continue;
            }

            // If controller's driver can't set its PIO mode, set the highest one the
            // controller supports
            if wdc.PIO_cap.get() >= i + 3 {
                drvp.PIO_mode.set(i + 3);
                drvp.PIO_cap.set(i + 3);
                break;
            }
        }
        if !valid_mode_found {
            // We didn't find a valid PIO mode. Assume the values returned for DMA are buggy
            // too
            return;
        }
        drvp.set(DRIVE_MODE);

        // Some controllers don't support ATAPI DMA
        if drvp.isset(DRIVE_ATAPI) && wdc.has(WDC_CAPABILITY_NO_ATAPI_DMA) {
            return;
        }

        for i in (0..=7u8).rev() {
            if params.atap_dmamode_supp & (1 << i) == 0 {
                continue;
            }
            if wdc.has(WDC_CAPABILITY_DMA)
                && wdc.has(WDC_CAPABILITY_MODE)
                && ata_set_mode(drvp, 0x20 | i, poll) != CMD_OK
            {
                continue;
            }

            if wdc.has(WDC_CAPABILITY_DMA) {
                if wdc.has(WDC_CAPABILITY_MODE) && wdc.DMA_cap.get() < i {
                    continue;
                }
                drvp.DMA_mode.set(i);
                drvp.DMA_cap.set(i);
                drvp.set(DRIVE_DMA);
            }
            break;
        }
        if params.atap_extensions & WDC_EXT_UDMA_MODES != 0 {
            for i in (0..=7u8).rev() {
                if params.atap_udmamode_supp & (1 << i) == 0 {
                    continue;
                }
                if wdc.has(WDC_CAPABILITY_MODE)
                    && wdc.has(WDC_CAPABILITY_UDMA)
                    && ata_set_mode(drvp, 0x40 | i, poll) != CMD_OK
                {
                    continue;
                }
                if wdc.has(WDC_CAPABILITY_UDMA) {
                    if wdc.has(WDC_CAPABILITY_MODE) && wdc.UDMA_cap.get() < i {
                        continue;
                    }
                    drvp.UDMA_mode.set(i);
                    drvp.UDMA_cap.set(i);
                    drvp.set(DRIVE_UDMA);
                }
                break;
            }
        }
    }

    // Try to guess ATA version here, if it didn't get reported
    if drvp.ata_vers.get() == 0 {
        if drvp.isset(DRIVE_UDMA) {
            drvp.ata_vers.set(4); // should be at last ATA-4
        } else if drvp.PIO_cap.get() > 2 {
            drvp.ata_vers.set(2); // should be at last ATA-2
        }
    }
    if cf_flags & ATA_CONFIG_PIO_SET != 0 {
        drvp.PIO_mode
            .set(((cf_flags & ATA_CONFIG_PIO_MODES) >> ATA_CONFIG_PIO_OFF) as u8);
        drvp.set(DRIVE_MODE);
    }
    if !wdc.has(WDC_CAPABILITY_DMA) {
        // don't care about DMA modes
        return;
    }
    if cf_flags & ATA_CONFIG_DMA_SET != 0 {
        if cf_flags & ATA_CONFIG_DMA_MODES == ATA_CONFIG_DMA_DISABLE {
            drvp.clr(DRIVE_DMA);
        } else {
            drvp.DMA_mode
                .set(((cf_flags & ATA_CONFIG_DMA_MODES) >> ATA_CONFIG_DMA_OFF) as u8);
            drvp.set(DRIVE_DMA | DRIVE_MODE);
        }
    }
    if !wdc.has(WDC_CAPABILITY_UDMA) {
        // don't care about UDMA modes
        return;
    }
    if cf_flags & ATA_CONFIG_UDMA_SET != 0 {
        if cf_flags & ATA_CONFIG_UDMA_MODES == ATA_CONFIG_UDMA_DISABLE {
            drvp.clr(DRIVE_UDMA);
        } else {
            drvp.UDMA_mode
                .set(((cf_flags & ATA_CONFIG_UDMA_MODES) >> ATA_CONFIG_UDMA_OFF) as u8);
            drvp.set(DRIVE_UDMA | DRIVE_MODE);
        }
    }
}

/// `wdc_output_bytes`: `buflen` bytes of `bytes` (zeros for `None`) to the drive's data
/// register, 32 bits at a time where the drive allows it.
pub fn wdc_output_bytes(drvp: &AtaDriveDatas, bytes: Option<&[u8]>, buflen: u32) {
    let chp = wdc_drvp_chp(drvp);
    let mut off = 0usize;
    let mut len = buflen;

    if drvp.isset(DRIVE_CAP32) {
        let roundlen = len & !3;

        chp.write_raw_multi_4(bytes.map(|b| &b[off.min(b.len())..]), roundlen);

        off += roundlen as usize;
        len -= roundlen;
    }

    if len > 0 {
        let roundlen = (len + 1) & !0x1;

        chp.write_raw_multi_2(bytes.map(|b| &b[off.min(b.len())..]), roundlen);
    }
}

/// `wdc_input_bytes`: `buflen` bytes from the drive's data register into `bytes` (or the
/// bit bucket for `None`), 32 bits at a time where the drive allows it.
pub fn wdc_input_bytes(drvp: &AtaDriveDatas, mut bytes: Option<&mut [u8]>, buflen: u32) {
    let chp = wdc_drvp_chp(drvp);
    let mut off = 0usize;
    let mut len = buflen;

    if drvp.isset(DRIVE_CAP32) {
        let roundlen = len & !3;

        chp.read_raw_multi_4(
            bytes.as_deref_mut().map(|b| {
                let n = b.len();
                &mut b[off.min(n)..]
            }),
            roundlen,
        );

        off += roundlen as usize;
        len -= roundlen;
    }

    if len > 0 {
        let roundlen = (len + 1) & !0x1;

        chp.read_raw_multi_2(
            bytes.map(|b| {
                let n = b.len();
                &mut b[off.min(n)..]
            }),
            roundlen,
        );
    }
}

/// `wdc_print_caps`: prints nothing: "This is actually a lie until we fix the _probe_caps
/// algorithm. Don't print out lies" (the C's message is `#if 0`).
pub fn wdc_print_caps(_drvp: &AtaDriveDatas) {
    // #if 0: "%s: can use " drive_name, "32-bit"/"16-bit", ", PIO mode %d" PIO_cap,
    // ", DMA mode %d" DMA_cap, ", Ultra-DMA mode %d" UDMA_cap, "\n".
}

/// `wdc_print_current_modes`: the modes the channel's drives use.
pub fn wdc_print_current_modes(chp: &ChannelSoftc) {
    let wdc = chp.wdc();
    for (drive, drvp) in chp.ch_drive.iter().enumerate() {
        if !drvp.isset(DRIVE) {
            continue;
        }

        let _ = printf(format_args!(
            "{}({}:{}:{}):",
            drvp.name(),
            wdc.sc_dev.xname(),
            chp.channel.get(),
            drive
        ));

        if !wdc.has(WDC_CAPABILITY_MODE) && drvp.cf_flags.get() & ATA_CONFIG_PIO_SET == 0 {
            let _ = printf(format_args!(" using BIOS timings"));
        } else {
            let _ = printf(format_args!(" using PIO mode {}", drvp.PIO_mode.get()));
        }
        if drvp.isset(DRIVE_DMA) {
            let _ = printf(format_args!(", DMA mode {}", drvp.DMA_mode.get()));
        }
        if drvp.isset(DRIVE_UDMA) {
            let _ = printf(format_args!(", Ultra-DMA mode {}", drvp.UDMA_mode.get()));
        }
        let _ = printf(format_args!("\n"));
    }
}

/// `wdc_downgrade_mode`: downgrades the transfer mode of a drive after an error. True if
/// downgrade was possible.
pub fn wdc_downgrade_mode(drvp: &AtaDriveDatas) -> bool {
    let chp = wdc_drvp_chp(drvp);
    let wdc = chp.wdc();
    let cf_flags = drvp.cf_flags.get();

    // if drive or controller don't know its mode, we can't do much
    if !drvp.isset(DRIVE_MODE) || !wdc.has(WDC_CAPABILITY_MODE) {
        return false;
    }
    // current drive mode was set by a config flag, let it this way
    if cf_flags & (ATA_CONFIG_PIO_SET | ATA_CONFIG_DMA_SET | ATA_CONFIG_UDMA_SET) != 0 {
        return false;
    }

    // We'd ideally like to use an Ultra DMA mode since they have the protection of a CRC.
    // So we try each Ultra DMA mode and see if we can find any working combo
    if drvp.isset(DRIVE_UDMA) && drvp.UDMA_mode.get() > 0 {
        drvp.UDMA_mode.set(drvp.UDMA_mode.get() - 1);
        let _ = printf(format_args!(
            "{}: transfer error, downgrading to Ultra-DMA mode {}\n",
            drvp.name(),
            drvp.UDMA_mode.get()
        ));
    } else if drvp.isset(DRIVE_UDMA) && !drvp.isset(DRIVE_DMAERR) {
        // If we were using ultra-DMA, don't downgrade to multiword DMA if we noticed a CRC
        // error. It has been noticed that CRC errors in ultra-DMA lead to silent data
        // corruption in multiword DMA. Data corruption is less likely to occur in PIO mode.
        drvp.clr(DRIVE_UDMA);
        drvp.set(DRIVE_DMA);
        drvp.DMA_mode.set(drvp.DMA_cap.get());
        let _ = printf(format_args!(
            "{}: transfer error, downgrading to DMA mode {}\n",
            drvp.name(),
            drvp.DMA_mode.get()
        ));
    } else if drvp.isset(DRIVE_DMA | DRIVE_UDMA) {
        drvp.clr(DRIVE_DMA | DRIVE_UDMA);
        drvp.PIO_mode.set(drvp.PIO_cap.get());
        let _ = printf(format_args!(
            "{}: transfer error, downgrading to PIO mode {}\n",
            drvp.name(),
            drvp.PIO_mode.get()
        ));
    } else {
        // already using PIO, can't downgrade
        return false;
    }

    match wdc.set_modes.get() {
        Some(set_modes) => set_modes(chp),
        None => panic(format_args!("{}: no set_modes", wdc.sc_dev.xname())),
    }
    // reset the channel, which will schedule all drives for setup
    wdc_reset_channel(drvp, false);
    true
}

/// `wdc_exec_command`: queues the short command `wdc_c` on the drive's channel and, with
/// `AT_WAIT`, sleeps until it is done (`AT_POLL` completes it before the queueing returns).
/// Returns `WDC_COMPLETE`, `WDC_QUEUED` or `WDC_TRY_AGAIN` (no transfer free).
///
/// # Safety
///
/// `wdc_c` stays valid and in place until `AT_DONE` is set in its flags; this holds when it
/// has `AT_WAIT` or `AT_POLL` (the command is done when this returns `WDC_COMPLETE`).
pub unsafe fn wdc_exec_command(drvp: &AtaDriveDatas, wdc_c: &WdcCommand) -> i32 {
    let chp = wdc_drvp_chp(drvp);

    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_FUNCS,
        "wdc_exec_command {}:{}:{}\n",
        chp.wdc().sc_dev.xname(),
        chp.channel.get(),
        drvp.drive.get()
    );

    // set up an xfer and queue. Wait for completion
    let Some(xfer) = wdc_get_xfer(if wdc_c.isset(AT_WAIT) {
        crate::dev::ic::wdcvar::WDC_CANSLEEP
    } else {
        WDC_NOSLEEP
    }) else {
        return WDC_TRY_AGAIN;
    };

    if wdc_c.isset(AT_POLL) {
        xfer.set(C_POLL);
    }
    xfer.drive.set(drvp.drive.get());
    xfer.databuf.set(wdc_c.data.get());
    xfer.c_bcount.set(wdc_c.bcount.get());
    xfer.cmd.set(ptr::from_ref(wdc_c).cast_mut().cast());
    xfer.c_start.set(Some(__wdccommand_start));
    xfer.c_intr.set(Some(__wdccommand_intr));
    xfer.c_kill_xfer.set(Some(__wdccommand_done));

    let s = splbio();
    wdc_exec_xfer(chp, xfer);
    #[cfg(feature = "diagnostic")]
    if wdc_c.isset(AT_POLL) && !wdc_c.isset(AT_DONE) {
        panic(format_args!("wdc_exec_command: polled command not done"));
    }
    let ret = if wdc_c.isset(AT_DONE) {
        WDC_COMPLETE
    } else if wdc_c.isset(AT_WAIT) {
        wdcdebug_print!(wdcdebug_mask, DEBUG_FUNCS, "wdc_exec_command sleeping\n");

        while !wdc_c.isset(AT_DONE) {
            let _ = tsleep_nsec(ptr::from_ref(wdc_c), PRIBIO, "wdccmd", INFSLP);
        }
        WDC_COMPLETE
    } else {
        WDC_QUEUED
    };
    splx(s);
    ret
}

/// The `struct wdc_command` of a transfer `wdc_exec_command` queued (`xfer->cmd`).
fn xfer_command(xfer: &WdcXfer) -> &WdcCommand {
    // SAFETY: `wdc_exec_command` sets `cmd` to the caller's command, which its contract
    // keeps alive until the command is done; these hooks only run until then.
    unsafe { &*xfer.cmd.get().cast::<WdcCommand>().cast_const() }
}

/// `__wdccommand_start`: the `c_start` of a short command.
pub fn __wdccommand_start(chp: &ChannelSoftc, xfer: &WdcXfer) {
    let drive = i32::from(xfer.drive.get());
    let wdc_c = xfer_command(xfer);

    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_FUNCS,
        "__wdccommand_start {}:{}:{}\n",
        chp.wdc().sc_dev.xname(),
        chp.channel.get(),
        drive
    );

    // Disable interrupts if we're polling
    if xfer.isset(C_POLL) {
        wdc_disable_intr(chp);
    }

    wdc_set_drive(chp, drive);
    delay(1);

    'timeout: {
        // For resets, we don't really care to make sure that the bus is free
        if wdc_c.r_command.get() != ATAPI_SOFT_RESET {
            if wdcwait(
                chp,
                wdc_c.r_st_bmask.get() | WDCS_DRQ,
                wdc_c.r_st_bmask.get(),
                wdc_c.timeout.get(),
            )
            .is_err()
            {
                break 'timeout;
            }
        } else {
            delay(10);
        }

        wdccommand(
            chp,
            drive as u8,
            wdc_c.r_command.get(),
            wdc_c.r_cyl.get(),
            wdc_c.r_head.get(),
            wdc_c.r_sector.get(),
            wdc_c.r_count.get(),
            wdc_c.r_features.get(),
        );

        if wdc_c.flags.get() & AT_WRITE == AT_WRITE {
            // wait at least 400ns before reading status register
            delay(10);
            if wait_for_unbusy(chp, wdc_c.timeout.get()).is_err() {
                break 'timeout;
            }

            if chp.ch_status.get() & (WDCS_DRQ | WDCS_ERR) == WDCS_ERR {
                __wdccommand_done(chp, xfer);
                return;
            }

            if wait_for_drq(chp, wdc_c.timeout.get()).is_err() {
                break 'timeout;
            }

            // SAFETY: the command's buffer is its caller's for the command's life
            // (`wdc_exec_command`'s contract), read here only.
            let data = unsafe { wdc_c.data_mut() };
            wdc_output_bytes(
                &chp.ch_drive[drive as usize],
                data.map(|d| &*d),
                wdc_c.bcount.get() as u32,
            );
        }

        if !wdc_c.isset(AT_POLL) {
            chp.set(WDCF_IRQ_WAIT); // wait for interrupt
            let _ = timeout_add_msec(&chp.ch_timo, wdc_c.timeout.get() as u64);
            return;
        }

        // Polled command. Wait for drive ready or drq. Done in intr(). Wait for at last 400ns
        // for status bit to be valid.
        delay(10);
        let _ = __wdccommand_intr(chp, xfer, 0);
        return;
    }

    // timeout:
    wdc_c.set(AT_TIMEOU);
    __wdccommand_done(chp, xfer);
}

/// `__wdccommand_intr`: the `c_intr` of a short command.
pub fn __wdccommand_intr(chp: &ChannelSoftc, xfer: &WdcXfer, irq: i32) -> i32 {
    let drvp = &chp.ch_drive[usize::from(xfer.drive.get())];
    let wdc_c = xfer_command(xfer);
    let bcount = wdc_c.bcount.get();

    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_INTR,
        "__wdccommand_intr {}:{}:{}\n",
        chp.wdc().sc_dev.xname(),
        chp.channel.get(),
        xfer.drive.get()
    );
    'out: {
        if wdcwait(
            chp,
            wdc_c.r_st_pmask.get(),
            wdc_c.r_st_pmask.get(),
            if irq == 0 { wdc_c.timeout.get() } else { 0 },
        )
        .is_err()
        {
            if chp.dying.get() != 0 {
                __wdccommand_done(chp, xfer);
                return -1;
            }
            if irq != 0 && !xfer.isset(C_TIMEOU) {
                return 0; // IRQ was not for us
            }
            wdc_c.set(AT_TIMEOU);
            break 'out;
        }
        let wdc = chp.wdc();
        if wdc.has(WDC_CAPABILITY_IRQACK) {
            irqack(chp);
        }
        if wdc_c.isset(AT_READ) {
            if chp.ch_status.get() & WDCS_DRQ == 0 {
                wdc_c.set(AT_TIMEOU);
                break 'out;
            }
            // SAFETY: the command's buffer is its caller's for the command's life
            // (`wdc_exec_command`'s contract); nothing else touches it while the drive
            // fills it.
            let data = unsafe { wdc_c.data_mut() };
            wdc_input_bytes(drvp, data, bcount as u32);
            // Should we wait for device to indicate idle?
        }
    }
    // out:
    __wdccommand_done(chp, xfer);
    wdcdebug_print!(wdcdebug_mask, DEBUG_INTR, "__wdccommand_intr returned\n");
    1
}

/// `chp->wdc->irqack(chp)`.
fn irqack(chp: &ChannelSoftc) {
    let wdc = chp.wdc();
    match wdc.irqack.get() {
        Some(f) => f(chp),
        None => panic(format_args!("{}: no irqack", wdc.sc_dev.xname())),
    }
}

/// `__wdccommand_done`: completes a short command (also its `c_kill_xfer`).
pub fn __wdccommand_done(chp: &ChannelSoftc, xfer: &WdcXfer) {
    let wdc_c = xfer_command(xfer);

    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_FUNCS,
        "__wdccommand_done {}:{}:{} {:02x}\n",
        chp.wdc().sc_dev.xname(),
        chp.channel.get(),
        xfer.drive.get(),
        chp.ch_status.get()
    );
    'killit: {
        if chp.dying.get() != 0 {
            break 'killit;
        }
        if chp.ch_status.get() & WDCS_DWF != 0 {
            wdc_c.set(AT_DF);
        }
        if chp.ch_status.get() & WDCS_ERR != 0 {
            wdc_c.set(AT_ERROR);
            wdc_c.r_error.set(chp.ch_error.get());
        }
        wdc_c.set(AT_DONE);
        if wdc_c.isset(AT_READREG) && !wdc_c.isset(AT_ERROR | AT_DF) {
            wdc_c.r_head.set(chp.read_reg(wdr_sdh));
            let mut cyl = u16::from(chp.read_reg(wdr_cyl_hi)) << 8;
            cyl |= u16::from(chp.read_reg(wdr_cyl_lo));
            wdc_c.r_cyl.set(cyl);
            wdc_c.r_sector.set(chp.read_reg(wdr_sector));
            wdc_c.r_count.set(chp.read_reg(wdr_seccnt));
            wdc_c.r_error.set(chp.read_reg(wdr_error));
            wdc_c.r_features.set(wdc_c.r_error.get());
        }
    }

    // killit:
    if xfer.isset(C_POLL) {
        wdc_enable_intr(chp);
    } else {
        let _ = timeout_del(&chp.ch_timo);
    }

    wdc_free_xfer(chp, xfer);
    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_INTR,
        "__wdccommand_done before callback\n"
    );

    if chp.dying.get() != 0 {
        return;
    }

    if wdc_c.isset(AT_WAIT) {
        wakeup(ptr::from_ref(wdc_c));
    } else if let Some(callback) = wdc_c.callback.get() {
        callback(wdc_c.callback_arg.get());
    }
    wdcstart(chp);
    wdcdebug_print!(wdcdebug_mask, DEBUG_INTR, "__wdccommand_done returned\n");
}

/// `wdccommand`: sends a command. The drive should be ready. Assumes interrupts are
/// blocked.
#[allow(clippy::too_many_arguments)] // the C function's eight arguments
pub fn wdccommand(
    chp: &ChannelSoftc,
    drive: u8,
    command: u8,
    cylin: u16,
    head: u8,
    sector: u8,
    count: u8,
    features: u8,
) {
    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_FUNCS,
        "wdccommand {}:{}:{}: command=0x{:x} cylin={} head={} sector={} count={} features={}\n",
        chp.wdc().sc_dev.xname(),
        chp.channel.get(),
        drive,
        command,
        cylin,
        head,
        sector,
        count,
        features
    );
    WDC_LOG_ATA_CMDLONG(
        chp,
        head,
        features,
        cylin as u8,
        (cylin >> 8) as u8,
        sector,
        count,
        command,
    );

    // Select drive, head, and addressing mode.
    chp.write_reg(wdr_sdh, WDSD_IBM | (drive << 4) | head);

    // Load parameters.
    chp.write_reg(wdr_features, features);
    chp.write_reg(wdr_cyl_lo, cylin as u8);
    chp.write_reg(wdr_cyl_hi, (cylin >> 8) as u8);
    chp.write_reg(wdr_sector, sector);
    chp.write_reg(wdr_seccnt, count);

    // Send command.
    chp.write_reg(wdr_command, command);
}

/// `wdccommandext`: sends a 48-bit addressing command. The drive should be ready. Assumes
/// interrupts are blocked.
pub fn wdccommandext(chp: &ChannelSoftc, drive: u8, command: u8, blkno: u64, count: u16) {
    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_FUNCS,
        "wdccommandext {}:{}:{}: command=0x{:x} blkno={} count={}\n",
        chp.wdc().sc_dev.xname(),
        chp.channel.get(),
        drive,
        command,
        blkno,
        count
    );
    WDC_LOG_ATA_CMDEXT(
        chp,
        (blkno >> 40) as u8,
        (blkno >> 16) as u8,
        (blkno >> 32) as u8,
        (blkno >> 8) as u8,
        (blkno >> 24) as u8,
        blkno as u8,
        (count >> 8) as u8,
        count as u8,
        command,
    );

    // Select drive and LBA mode.
    chp.write_reg(wdr_sdh, (drive << 4) | WDSD_LBA);

    // Load parameters.
    chp.lba48_write_reg(
        wdr_lba_hi,
        (((blkno >> 32) & 0xff00) | ((blkno >> 16) & 0xff)) as u16,
    );
    chp.lba48_write_reg(
        wdr_lba_mi,
        (((blkno >> 24) & 0xff00) | ((blkno >> 8) & 0xff)) as u16,
    );
    chp.lba48_write_reg(
        wdr_lba_lo,
        (((blkno >> 16) & 0xff00) | (blkno & 0xff)) as u16,
    );
    chp.lba48_write_reg(wdr_seccnt, count);

    // Send command.
    chp.write_reg(wdr_command, command);
}

/// `wdccommandshort`: simplified version of `wdccommand()`. Unbusy/ready/drq must be tested
/// by the caller.
pub fn wdccommandshort(chp: &ChannelSoftc, drive: i32, command: u8) {
    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_FUNCS,
        "wdccommandshort {}:{}:{} command 0x{:x}\n",
        chp.wdc().sc_dev.xname(),
        chp.channel.get(),
        drive,
        command
    );
    WDC_LOG_ATA_CMDSHORT(chp, command);

    // Select drive.
    chp.write_reg(wdr_sdh, WDSD_IBM | ((drive << 4) as u8));
    chp.write_reg(wdr_command, command);
}

/// `wdc_exec_xfer`: adds a command to the queue and starts the controller. Must be called at
/// splbio.
pub fn wdc_exec_xfer(chp: &ChannelSoftc, xfer: &WdcXfer) {
    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_XFERS,
        "wdc_exec_xfer {:p} flags 0x{:x} channel {} drive {}\n",
        xfer,
        xfer.c_flags.get(),
        chp.channel.get(),
        xfer.drive.get()
    );

    // complete xfer setup
    xfer.chp.set(Some(NonNull::from(chp)));

    let queue = chp.queue();
    // If we are a polled command, and the list is not empty, we are doing a dump. Drop the
    // list to allow the polled command to complete, we're going to reboot soon anyway.
    if xfer.isset(C_POLL) && !queue.sc_xfer.is_empty() {
        queue.sc_xfer.init();
    }
    // insert at the end of command list
    // SAFETY: `xfer` is a new transfer (from `wdc_get_xfer`, or the caller's private one),
    // in no queue; it stays in place until `wdc_free_xfer` unlinks it. The queue head lives
    // in its `wdc_alloc_queue` allocation.
    unsafe { queue.sc_xfer.insert_tail(xfer) };
    wdcdebug_print!(
        wdcdebug_mask,
        DEBUG_XFERS,
        "wdcstart from wdc_exec_xfer, flags 0x{:x}\n",
        chp.ch_flags.get()
    );
    wdcstart(chp);
}

/// `wdc_xfer_get`: the iopool's `io_get`, a zeroed transfer from `wdc_xfer_pool`.
///
/// # Safety
///
/// None beyond [`ScsiIoGetFn`](crate::scsi::scsiconf::ScsiIoGetFn)'s; the cookie is unused.
unsafe fn wdc_xfer_get(_null: *mut c_void) -> Option<ScsiIo> {
    pool_get(&WDC_XFER_POOL, PR_NOWAIT | PR_ZERO).map(NonNull::cast)
}

/// `wdc_scrub_xfer`: clears a transfer the SCSI layer keeps for reuse.
///
/// # Safety
///
/// `xfer` is a transfer of this pool in no queue, and no reference to it is live.
pub unsafe fn wdc_scrub_xfer(xfer: NonNull<WdcXfer>) {
    // SAFETY: the caller's contract; every member of a transfer is valid as all zero bits.
    unsafe {
        ptr::write_bytes(xfer.as_ptr(), 0, 1);
        xfer.as_ref().c_flags.set(C_SCSIXFER);
    }
}

/// `wdc_xfer_put`: the iopool's `io_put`, which keeps a transfer the SCSI layer still
/// owns (`C_SCSIXFER`) and gives the others back to the pool.
///
/// # Safety
///
/// `xxfer` came from [`wdc_xfer_get`] and is in no queue.
unsafe fn wdc_xfer_put(_null: *mut c_void, xxfer: ScsiIo) {
    // SAFETY: the caller's contract: a transfer of the pool.
    let xfer = unsafe { xxfer.cast::<WdcXfer>().as_ref() };
    let mut put = false;

    let s = splbio();
    if xfer.isset(C_SCSIXFER) {
        xfer.clr(C_SCSIXFER);
    } else {
        put = true;
    }
    splx(s);

    if put {
        pool_put(&WDC_XFER_POOL, xxfer.cast());
    }
}

/// `wdc_get_xfer`: a transfer of the shared pool (`WDC_NOSLEEP` or `WDC_CANSLEEP`).
pub fn wdc_get_xfer(flags: i32) -> Option<&'static WdcXfer> {
    let io = scsi_io_get(
        &WDC_XFER_IOPOOL.0,
        if flags & WDC_NOSLEEP != 0 {
            SCSI_NOSLEEP
        } else {
            0
        },
    )?;
    // SAFETY: the iopool's openings are `wdc_xfer_get`'s transfers: pool items of a
    // `WdcXfer`'s size, zeroed (valid as all zero bits), alive until `wdc_free_xfer` gives
    // them back.
    Some(unsafe { io.cast::<WdcXfer>().as_ref() })
}

/// `wdc_free_xfer`: takes `xfer` off the channel's queue and gives it back.
pub fn wdc_free_xfer(chp: &ChannelSoftc, xfer: &WdcXfer) {
    let queue = chp.queue();
    let mut put = false;

    if xfer.isset(C_PRIVATEXFER) {
        chp.clr(WDCF_ACTIVE);
        // SAFETY: a private transfer is queued by `wdc_exec_xfer` until it completes here.
        unsafe { queue.sc_xfer.remove(xfer) };
        return;
    }

    let s = splbio();
    chp.clr(WDCF_ACTIVE);
    // SAFETY: the transfer being completed is the running one, on this queue.
    unsafe { queue.sc_xfer.remove(xfer) };
    if xfer.isset(C_SCSIXFER) {
        xfer.clr(C_SCSIXFER);
    } else {
        put = true;
    }
    splx(s);

    if put {
        scsi_io_put(&WDC_XFER_IOPOOL.0, NonNull::from(xfer).cast());
    }
}

/// `wdc_kill_pending`: kills off all pending xfers for a channel. Must be called at
/// `splbio()`.
pub fn wdc_kill_pending(chp: &ChannelSoftc) {
    let mut chp = chp;
    while let Some(xfer) = chp.queue().sc_xfer.first() {
        chp = xfer.chp();
        xfer.kill(chp);
    }
}

/// `__wdcerror`: a message about the channel, or its running transfer's drive.
pub fn __wdcerror(chp: &ChannelSoftc, msg: &str) {
    let wdc = chp.wdc();
    match chp.queue().sc_xfer.first() {
        None => {
            let _ = printf(format_args!(
                "{}:{}: {}\n",
                wdc.sc_dev.xname(),
                chp.channel.get(),
                msg
            ));
        }
        Some(xfer) => {
            let drive = xfer.drive.get();
            let _ = printf(format_args!(
                "{}({}:{}:{}): {}\n",
                chp.ch_drive[usize::from(drive)].name(),
                wdc.sc_dev.xname(),
                chp.channel.get(),
                drive,
                msg
            ));
        }
    }
}

/// `wdcbit_bucket`: the bit bucket, `size` bytes read from the data register and dropped.
pub fn wdcbit_bucket(chp: &ChannelSoftc, size: i32) {
    chp.read_raw_multi_2(None, size as u32);
}

/// `wdc_ioc_ata_cmd`: runs the ATA command of an `ATAIOCCOMMAND` request on the drive.
pub fn wdc_ioc_ata_cmd(drvp: &AtaDriveDatas, atareq: &mut Atareq) -> Result<(), Errno> {
    // Make sure a timeout was supplied in the ioctl request
    if atareq.timeout == 0 {
        return Err(EINVAL);
    }

    if atareq.datalen > MAXPHYS as u64 {
        return Err(EINVAL);
    }
    let datalen = atareq.datalen as usize;

    let wdc_c = WdcCommand::new();

    // The buffer, freed (dma_free) when it goes out of scope (`err:`).
    let mut buf = None;
    if datalen > 0 {
        let Some(mut b) = DmaBuf::new(datalen, M_NOWAIT) else {
            return Err(ENOMEM);
        };
        wdc_c.data.set(b.bytes().as_mut_ptr());
        wdc_c.bcount.set(datalen as i32);
        buf = Some(b);
    }

    wdc_c.flags.set(AT_WAIT);
    if atareq.flags & ATACMD_READ != 0 {
        wdc_c.set(AT_READ);
    }
    if atareq.flags & ATACMD_WRITE != 0 {
        if let Some(b) = buf.as_mut() {
            copyin(atareq.databuf, b.bytes())?;
        }
        wdc_c.set(AT_WRITE);
    }
    if atareq.flags & ATACMD_READREG != 0 {
        wdc_c.set(AT_READREG);
    }

    wdc_c.timeout.set(atareq.timeout);
    wdc_c.r_command.set(atareq.command);
    wdc_c.r_head.set(atareq.head & 0x0f);
    wdc_c.r_cyl.set(atareq.cylinder);
    wdc_c.r_sector.set(atareq.sec_num);
    wdc_c.r_count.set(atareq.sec_count);
    wdc_c.r_features.set(atareq.features);
    if drvp.isset(DRIVE_ATAPI) {
        if wdc_c.r_command.get() == WDCC_IDENTIFY {
            wdc_c.r_command.set(ATAPI_IDENTIFY_DEVICE);
        }
    } else {
        wdc_c.r_st_bmask.set(WDCS_DRDY);
        wdc_c.r_st_pmask.set(WDCS_DRDY);
    }

    // SAFETY: `AT_WAIT`: the command is done when this returns (or was never queued).
    if unsafe { wdc_exec_command(drvp, &wdc_c) } != WDC_COMPLETE {
        atareq.retsts = ATACMD_ERROR;
    } else if wdc_c.isset(AT_ERROR | AT_TIMEOU | AT_DF) {
        if wdc_c.isset(AT_ERROR) {
            atareq.retsts = ATACMD_ERROR;
            atareq.error = wdc_c.r_error.get();
        } else if wdc_c.isset(AT_DF) {
            atareq.retsts = ATACMD_DF;
        } else {
            atareq.retsts = ATACMD_TIMEOUT;
        }
    } else {
        atareq.retsts = ATACMD_OK;
        if atareq.flags & ATACMD_READREG != 0 {
            atareq.head = wdc_c.r_head.get();
            atareq.cylinder = wdc_c.r_cyl.get();
            atareq.sec_num = wdc_c.r_sector.get();
            atareq.sec_count = wdc_c.r_count.get();
            atareq.features = wdc_c.r_features.get();
            atareq.error = wdc_c.r_error.get();
        }
    }

    // copyout:
    if atareq.flags & ATACMD_READ != 0
        && let Some(b) = buf.as_mut()
    {
        copyout(b.bytes(), atareq.databuf)?;
    }

    // err: the buffer is freed with `buf`.
    Ok(())
}

/// `wdc_ioctl`: the controller's share of a drive's ioctls (`ATAIOCCOMMAND`, and
/// `ATAIOGETTRACE` with `option WDCDEBUG`).
pub fn wdc_ioctl(
    drvp: &AtaDriveDatas,
    xfer: u64,
    addr: &mut [u8],
    flag: i32,
    _p: &Proc,
) -> Result<(), Errno> {
    match xfer {
        #[cfg(feature = "wdcdebug")]
        ATAIOGETTRACE => {
            let mut agt = ioctl_arg::<Atagettrace>(addr);
            let mut size = agt.buf_size;
            if size > 65536 {
                size = 65536;
            }

            let mut error = Ok(());
            let log_to_copy = wdc_get_log(&mut size, &mut agt.bytes_left);

            if let Some(log) = log_to_copy {
                // SAFETY: `wdc_get_log` filled `size` bytes of the buffer it returns.
                let bytes = unsafe { core::slice::from_raw_parts(log.as_ptr(), size as usize) };
                error = copyout(bytes, agt.buf);
                free(log, M_TEMP, 0);
            }

            agt.bytes_copied = size;
            ioctl_ret(addr, &agt);
            error
        }

        ATAIOCCOMMAND => {
            let mut atareq = ioctl_arg::<Atareq>(addr);

            // Make sure this command is (relatively) safe first
            let error = if flag & FWRITE == 0 && atareq.flags & ATACMD_WRITE != 0 {
                Err(EPERM)
            } else {
                wdc_ioc_ata_cmd(drvp, &mut atareq)
            };
            ioctl_ret(addr, &atareq);
            error
        }

        _ => Err(ENOTTY),
    }
}
/* </CODE> */
