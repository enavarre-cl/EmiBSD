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
 * Copyright (c) 1998, 2001 Manuel Bouyer.
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
//! High-level structures used by both ATA and ATAPI devices: the per-drive data a controller
//! and the disk driver share (`struct ata_drive_datas`), the attach arguments of a drive, the
//! configuration flags that force a transfer mode, and the short-command interface between a
//! drive driver and its controller (`struct wdc_command`).
//!
//! Upstream: sys/dev/ata/atavar.h @ 3ce1f3f79392
//!
//! The functions declared by the header live in `wdc.rs` (`wdc_exec_command`,
//! `wdc_probe_caps`, `wdc_print_caps`, `wdc_downgrade_mode`, `wdc_reset_channel`, `at_poll`)
//! and `ata.rs` (`ata_get_params`, `ata_set_mode`, `ata_dmaerr`, `ata_perror`).
//!
//! ## Deviations
//! - The members the controller and the drive driver change after attach are `Cell`s: a
//!   drive's data is shared by reference between its channel, the interrupt handler and the
//!   disk driver, as the C shares it by pointer, under `splbio`. `id`, the 512-byte IDENTIFY
//!   block, is an `UnsafeCell` read in place ([`AtaDriveDatas::id`]) and written only through
//!   [`AtaDriveDatas::id_mut`] by `wdcattach`.
//! - The mixed-case members (`PIO_mode`, `DMA_cap`, ...) keep their C names.
//! - `chnl_softc` stays untyped, as the C's `void *`: this header is the controller-neutral
//!   one; `wdc.rs` turns it back into its `struct channel_softc`.
//! - `struct wdc_command` is shared with the channel while the command runs (the transfer
//!   points at it and the interrupt handler completes it), so its members are `Cell`s; `data`
//!   stays a raw pointer to `bcount` bytes, as the buffer is the caller's.
//! - `aa_drv_data` is a reference to the drive's data, which lives in its channel for as long
//!   as the controller is attached.

#![allow(non_snake_case)] // PIO_mode, DMA_mode, UDMA_mode, PIO_cap, DMA_cap, UDMA_cap

use core::cell::{Cell, UnsafeCell};
use core::ffi::c_void;
use core::ptr::{self, NonNull};

use crate::dev::ata::atareg::Ataparams;
use crate::kern::subr_prf::Str;

/* Datas common to drives and controller drivers */

/// `DRIVE_ATA`.
pub const DRIVE_ATA: u16 = 0x0001;
/// `DRIVE_ATAPI`.
pub const DRIVE_ATAPI: u16 = 0x0002;
/// `DRIVE_OLD`.
pub const DRIVE_OLD: u16 = 0x0004;
/// `DRIVE`: any kind of drive.
pub const DRIVE: u16 = DRIVE_ATA | DRIVE_ATAPI | DRIVE_OLD;
/// `DRIVE_CAP32`.
pub const DRIVE_CAP32: u16 = 0x0008;
/// `DRIVE_DMA`.
pub const DRIVE_DMA: u16 = 0x0010;
/// `DRIVE_UDMA`.
pub const DRIVE_UDMA: u16 = 0x0020;
/// `DRIVE_MODE`: the drive reported its mode.
pub const DRIVE_MODE: u16 = 0x0040;
/// `DRIVE_RESET`: reset the drive state at next xfer.
pub const DRIVE_RESET: u16 = 0x0080;
/// `DRIVE_DMAERR`: Udma transfer had crc error, don't try DMA.
pub const DRIVE_DMAERR: u16 = 0x0100;
/// `DRIVE_DSCBA`: DSC in buffer availability mode.
pub const DRIVE_DSCBA: u16 = 0x0200;
/// `DRIVE_DSCWAIT`: in wait for DSC to be asserted.
pub const DRIVE_DSCWAIT: u16 = 0x0400;
/// `DRIVE_DEVICE_RESET`: drive supports DEVICE RESET command.
pub const DRIVE_DEVICE_RESET: u16 = 0x0800;
/// `DRIVE_SATA`: SATA drive.
pub const DRIVE_SATA: u16 = 0x1000;

/// `ACAP_LEN`: 16 byte commands.
pub const ACAP_LEN: u8 = 0x01;
/// `ACAP_DSC`: use DSC signalling.
pub const ACAP_DSC: u8 = 0x02;
// 0x20-0x40 reserved for ATAPI_CFG_DRQ_MASK

/// `NERRS_MAX`.
pub const NERRS_MAX: u8 = 4;
/// `NXFER`.
pub const NXFER: u32 = 1000;

/* ATA/ATAPI common attachment data */

/// `T_ATA`.
pub const T_ATA: u8 = 0;
/// `T_ATAPI`.
pub const T_ATAPI: u8 = 1;

/* User config flags that force (or disable) the use of a mode */

/// `ATA_CONFIG_PIO_MODES`.
pub const ATA_CONFIG_PIO_MODES: i32 = 0x0007;
/// `ATA_CONFIG_PIO_SET`.
pub const ATA_CONFIG_PIO_SET: i32 = 0x0008;
/// `ATA_CONFIG_PIO_OFF`.
pub const ATA_CONFIG_PIO_OFF: i32 = 0;
/// `ATA_CONFIG_DMA_MODES`.
pub const ATA_CONFIG_DMA_MODES: i32 = 0x0070;
/// `ATA_CONFIG_DMA_SET`.
pub const ATA_CONFIG_DMA_SET: i32 = 0x0080;
/// `ATA_CONFIG_DMA_DISABLE`.
pub const ATA_CONFIG_DMA_DISABLE: i32 = 0x0070;
/// `ATA_CONFIG_DMA_OFF`.
pub const ATA_CONFIG_DMA_OFF: i32 = 4;
/// `ATA_CONFIG_UDMA_MODES`.
pub const ATA_CONFIG_UDMA_MODES: i32 = 0x0700;
/// `ATA_CONFIG_UDMA_SET`.
pub const ATA_CONFIG_UDMA_SET: i32 = 0x0800;
/// `ATA_CONFIG_UDMA_DISABLE`.
pub const ATA_CONFIG_UDMA_DISABLE: i32 = 0x0700;
/// `ATA_CONFIG_UDMA_OFF`.
pub const ATA_CONFIG_UDMA_OFF: i32 = 8;

// The flags of a `struct wdc_command`.

/// `AT_READ`: there is data to read.
pub const AT_READ: u16 = 0x0001;
/// `AT_WRITE`: there is data to write (excl. with `AT_READ`).
pub const AT_WRITE: u16 = 0x0002;
/// `AT_WAIT`: wait in controller code for command completion.
pub const AT_WAIT: u16 = 0x0008;
/// `AT_POLL`: poll for command completion (no interrupts).
pub const AT_POLL: u16 = 0x0010;
/// `AT_DONE`: command is done.
pub const AT_DONE: u16 = 0x0020;
/// `AT_ERROR`: command is done with error.
pub const AT_ERROR: u16 = 0x0040;
/// `AT_TIMEOU`: command timed out.
pub const AT_TIMEOU: u16 = 0x0080;
/// `AT_DF`: drive fault.
pub const AT_DF: u16 = 0x0100;
/// `AT_READREG`: read registers on completion.
pub const AT_READREG: u16 = 0x0200;

// What `wdc_exec_command` returns.

/// `WDC_COMPLETE`.
pub const WDC_COMPLETE: i32 = 0x01;
/// `WDC_QUEUED`.
pub const WDC_QUEUED: i32 = 0x02;
/// `WDC_TRY_AGAIN`.
pub const WDC_TRY_AGAIN: i32 = 0x03;

/* return code for these cmds */

/// `CMD_OK`.
pub const CMD_OK: i32 = 0;
/// `CMD_ERR`.
pub const CMD_ERR: i32 = 1;
/// `CMD_AGAIN`.
pub const CMD_AGAIN: i32 = 2;

/// `struct ata_drive_datas`: datas common to drives and controller drivers.
pub struct AtaDriveDatas {
    /// `drive`: drive number.
    pub drive: Cell<u8>,
    /// `ata_vers`: ATA version supported.
    pub ata_vers: Cell<i8>,
    /// `drive_flags`: bitmask for drives present/absent and cap (`DRIVE_*`).
    pub drive_flags: Cell<u16>,
    // Current setting of drive's PIO, DMA and UDMA modes. Is initialised by the disks
    // drivers at attach time, and may be changed later by the controller's code if needed.
    /// `PIO_mode`: current setting of drive's PIO mode.
    pub PIO_mode: Cell<u8>,
    /// `DMA_mode`: current setting of drive's DMA mode.
    pub DMA_mode: Cell<u8>,
    /// `UDMA_mode`: current setting of drive's UDMA mode.
    pub UDMA_mode: Cell<u8>,
    // Supported modes for this drive.
    /// `PIO_cap`: supported drive's PIO mode.
    pub PIO_cap: Cell<u8>,
    /// `DMA_cap`: supported drive's DMA mode.
    pub DMA_cap: Cell<u8>,
    /// `UDMA_cap`: supported drive's UDMA mode.
    pub UDMA_cap: Cell<u8>,
    /// `state`: drive state. This is drive-type (ATA or ATAPI) dependant. This is reset to
    /// 0 after a channel reset.
    pub state: Cell<u8>,
    /// `atapi_cap` (`ACAP_*`).
    pub atapi_cap: Cell<u8>,
    /// `n_resets`: keeps track of the number of resets that have occurred in a row without
    /// a successful command completion.
    pub n_resets: Cell<u8>,
    /// `n_dmaerrs`.
    pub n_dmaerrs: Cell<u8>,
    /// `n_xfers`.
    pub n_xfers: Cell<u32>,
    /// `drive_name`: the attached driver's `dv_xname`, NUL-terminated (empty if none).
    pub drive_name: Cell<[u8; 31]>,
    /// `cf_flags`.
    pub cf_flags: Cell<i32>,
    /// `chnl_softc`: channel softc (`void *`).
    pub chnl_softc: Cell<*mut c_void>,
    /// `id`: the drive's IDENTIFY block.
    id: UnsafeCell<Ataparams>,
}

impl AtaDriveDatas {
    /// All zero, as in an `M_ZERO` softc.
    pub const fn new() -> Self {
        Self {
            drive: Cell::new(0),
            ata_vers: Cell::new(0),
            drive_flags: Cell::new(0),
            PIO_mode: Cell::new(0),
            DMA_mode: Cell::new(0),
            UDMA_mode: Cell::new(0),
            PIO_cap: Cell::new(0),
            DMA_cap: Cell::new(0),
            UDMA_cap: Cell::new(0),
            state: Cell::new(0),
            atapi_cap: Cell::new(0),
            n_resets: Cell::new(0),
            n_dmaerrs: Cell::new(0),
            n_xfers: Cell::new(0),
            drive_name: Cell::new([0; 31]),
            cf_flags: Cell::new(0),
            chnl_softc: Cell::new(ptr::null_mut()),
            id: UnsafeCell::new(Ataparams::zeroed()),
        }
    }

    /// `drvp->drive_flags & f`.
    pub fn isset(&self, f: u16) -> bool {
        self.drive_flags.get() & f != 0
    }

    /// `drvp->drive_flags |= f`.
    pub fn set(&self, f: u16) {
        self.drive_flags.set(self.drive_flags.get() | f);
    }

    /// `drvp->drive_flags &= ~f`.
    pub fn clr(&self, f: u16) {
        self.drive_flags.set(self.drive_flags.get() & !f);
    }

    /// `drvp->drive_name` for `%s`.
    pub fn name(&self) -> Str<'_> {
        // SAFETY: `drive_name` is written once, by the drive driver's attach (`wdattach`),
        // and cleared by `wdcattach` for drives nothing attached; neither happens while a
        // message about the drive is being printed. The drive's data outlives the messages.
        Str(unsafe { &*self.drive_name.as_ptr().cast_const() })
    }

    /// `&drvp->id`, read in place.
    pub fn id(&self) -> &Ataparams {
        // SAFETY: `id` is only written through `id_mut`, whose callers keep no other
        // reference live (its contract).
        unsafe { &*self.id.get() }
    }

    /// `&drvp->id`, writable (`ata_get_params(drvp, flags, &drvp->id)`).
    ///
    /// # Safety
    ///
    /// The caller is the channel's attach path (`wdcattach`), serialised against every other
    /// user of the block, and keeps no other reference to it while the returned one is live.
    #[allow(clippy::mut_from_ref)] // the C's struct member, shared by pointer
    pub unsafe fn id_mut(&self) -> &mut Ataparams {
        // SAFETY: the caller's contract.
        unsafe { &mut *self.id.get() }
    }
}

impl Default for AtaDriveDatas {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct ata_atapi_attach`: ATA/ATAPI common attachment data.
pub struct AtaAtapiAttach {
    /// `aa_type`: type of device (`T_ATA`, `T_ATAPI`).
    pub aa_type: u8,
    /// `aa_channel`: controller's channel.
    pub aa_channel: u8,
    /// `aa_openings`: number of simultaneous commands possible.
    pub aa_openings: u8,
    /// `aa_drv_data`.
    pub aa_drv_data: Option<&'static AtaDriveDatas>,
    /// `aa_bus_private`: info specific to this bus.
    pub aa_bus_private: *mut c_void,
}

/// `struct wdc_command`: ATA/ATAPI commands description.
///
/// This structure defines the interface between the ATA/ATAPI device driver and the
/// controller for short commands. It contains the command's parameter, the len of data's to
/// read/write (if any), and a function to call upon completion. If no sleep is allowed, the
/// driver can poll for command completion. Once the command completed, if the error register
/// is valid, the flag `AT_ERROR` is set and the error register value is copied to `r_error`.
/// A separate interface is needed for read/write or ATAPI packet commands (which need
/// multiple interrupts per commands).
pub struct WdcCommand {
    /// `r_command`: parameters to upload to registers.
    pub r_command: Cell<u8>,
    /// `r_head`.
    pub r_head: Cell<u8>,
    /// `r_cyl`.
    pub r_cyl: Cell<u16>,
    /// `r_sector`.
    pub r_sector: Cell<u8>,
    /// `r_count`.
    pub r_count: Cell<u8>,
    /// `r_features`.
    pub r_features: Cell<u8>,
    /// `r_st_bmask`: status register mask to wait for before command.
    pub r_st_bmask: Cell<u8>,
    /// `r_st_pmask`: status register mask to wait for after command.
    pub r_st_pmask: Cell<u8>,
    /// `r_error`: error register after command done.
    pub r_error: Cell<u8>,
    /// `flags` (`AT_*`; `volatile` in the C).
    pub flags: Cell<u16>,
    /// `timeout`: timeout (in ms).
    pub timeout: Cell<i32>,
    /// `data`: data buffer address.
    pub data: Cell<*mut u8>,
    /// `bcount`: number of bytes to transfer.
    pub bcount: Cell<i32>,
    /// `callback`: command to call once command completed.
    pub callback: Cell<Option<fn(*mut c_void)>>,
    /// `callback_arg`: argument passed to `*callback()`.
    pub callback_arg: Cell<*mut c_void>,
}

impl WdcCommand {
    /// `bzero(&wdc_c, sizeof(struct wdc_command))`.
    pub const fn new() -> Self {
        Self {
            r_command: Cell::new(0),
            r_head: Cell::new(0),
            r_cyl: Cell::new(0),
            r_sector: Cell::new(0),
            r_count: Cell::new(0),
            r_features: Cell::new(0),
            r_st_bmask: Cell::new(0),
            r_st_pmask: Cell::new(0),
            r_error: Cell::new(0),
            flags: Cell::new(0),
            timeout: Cell::new(0),
            data: Cell::new(ptr::null_mut()),
            bcount: Cell::new(0),
            callback: Cell::new(None),
            callback_arg: Cell::new(ptr::null_mut()),
        }
    }

    /// `wdc_c->flags |= f`.
    pub fn set(&self, f: u16) {
        self.flags.set(self.flags.get() | f);
    }

    /// `wdc_c->flags & f`.
    pub fn isset(&self, f: u16) -> bool {
        self.flags.get() & f != 0
    }

    /// `wdc_c->data` as a byte slice of `bcount` bytes (`None` for no buffer).
    ///
    /// # Safety
    ///
    /// `data` is NULL or points to `bcount` bytes the caller of `wdc_exec_command` owns for
    /// the command's life, which nothing else touches while the slice is live.
    #[allow(clippy::mut_from_ref)] // the C's `void *data`, shared with the channel
    pub unsafe fn data_mut(&self) -> Option<&mut [u8]> {
        let n = usize::try_from(self.bcount.get()).unwrap_or(0);
        // SAFETY: the caller's contract.
        NonNull::new(self.data.get())
            .map(|p| unsafe { core::slice::from_raw_parts_mut(p.as_ptr(), n) })
    }
}

impl Default for WdcCommand {
    fn default() -> Self {
        Self::new()
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_flags_helpers() {
        let d = AtaDriveDatas::new();
        d.set(DRIVE_ATA | DRIVE_OLD);
        assert!(d.isset(DRIVE));
        d.clr(DRIVE_OLD);
        assert_eq!(d.drive_flags.get(), DRIVE_ATA);
        let mut name = [0u8; 31];
        name[..3].copy_from_slice(b"wd0");
        d.drive_name.set(name);
        assert_eq!(std::format!("{}", d.name()), "wd0");
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ata/atavar.h");
        let names = crate::reftest::assert_defines!(defs;
            DRIVE_ATA, DRIVE_ATAPI, DRIVE_OLD, DRIVE, DRIVE_CAP32, DRIVE_DMA, DRIVE_UDMA,
            DRIVE_MODE, DRIVE_RESET, DRIVE_DMAERR, DRIVE_DSCBA, DRIVE_DSCWAIT,
            DRIVE_DEVICE_RESET, DRIVE_SATA, ACAP_LEN, ACAP_DSC, NERRS_MAX, NXFER, T_ATA,
            T_ATAPI, ATA_CONFIG_PIO_MODES, ATA_CONFIG_PIO_SET, ATA_CONFIG_PIO_OFF,
            ATA_CONFIG_DMA_MODES, ATA_CONFIG_DMA_SET, ATA_CONFIG_DMA_DISABLE, ATA_CONFIG_DMA_OFF,
            ATA_CONFIG_UDMA_MODES, ATA_CONFIG_UDMA_SET, ATA_CONFIG_UDMA_DISABLE,
            ATA_CONFIG_UDMA_OFF, AT_READ, AT_WRITE, AT_WAIT, AT_POLL, AT_DONE, AT_ERROR,
            AT_TIMEOU, AT_DF, AT_READREG, WDC_COMPLETE, WDC_QUEUED, WDC_TRY_AGAIN, CMD_OK,
            CMD_ERR, CMD_AGAIN,
        );
        for prefix in [
            "DRIVE",
            "ACAP_",
            "T_",
            "ATA_CONFIG_",
            "AT_",
            "WDC_",
            "CMD_",
            "N",
        ] {
            crate::reftest::assert_complete(&defs, prefix, &names);
        }
    }
}
/* </TESTS> */
