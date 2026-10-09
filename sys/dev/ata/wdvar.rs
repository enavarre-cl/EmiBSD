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
//! wd(4)'s softc and the request it hands its controller: `struct ata_bio`, the parameters a
//! controller needs to perform one ATA block transfer, and the drive states the transfer
//! code walks through before the drive is ready.
//!
//! Upstream: sys/dev/ata/wdvar.h @ 3ce1f3f79392
//!
//! `wdc_ata_bio` is in `ata_wdc.rs`, `wddone` in `wd.rs`.
//!
//! ## Deviations
//! - The members changed after attach are `Cell`s: the softc and its `ata_bio` are shared by
//!   reference between the block device entry points, the channel's interrupt handler and the
//!   restart timeout, under `splbio`, as the C shares them by pointer. `sc_params` is an
//!   `UnsafeCell` read in place ([`WdSoftc::params`]) and written through
//!   [`WdSoftc::params_mut`].
//! - `ata_bio`'s `lp` points at the disk's in-core label, read in place by the transfer code
//!   ([`AtaBio::label`]); `databuf` stays a raw pointer to the buffer's `bcount` bytes; `wd`
//!   points back at the softc.
//! - `wd_hibernate_io` is declared for `option HIBERNATE`, which is not configured: it is not
//!   compiled (`ata_wdc.rs`).

use core::cell::{Cell, UnsafeCell};
use core::ptr::{self, NonNull};

use crate::dev::ata::atareg::Ataparams;
use crate::dev::ata::atavar::AtaDriveDatas;
use crate::kern::subr_prf::panic;
use crate::sys::buf::{Buf, Bufq};
use crate::sys::device::{Device, Softc};
use crate::sys::disk::Disk;
use crate::sys::disklabel::Disklabel;
use crate::sys::timeout::Timeout;
use crate::sys::types::Daddr;

/// `ATA_NOSLEEP`: can't sleep.
pub const ATA_NOSLEEP: u16 = 0x0001;
/// `ATA_POLL`: poll for completion.
pub const ATA_POLL: u16 = 0x0002;
/// `ATA_ITSDONE`: the transfer is as done as it gets.
pub const ATA_ITSDONE: u16 = 0x0004;
/// `ATA_SINGLE`: transfer has to be done in single-sector mode.
pub const ATA_SINGLE: u16 = 0x0008;
/// `ATA_LBA`: transfer uses LBA addressing.
pub const ATA_LBA: u16 = 0x0010;
/// `ATA_READ`: transfer is a read (otherwise a write).
pub const ATA_READ: u16 = 0x0020;
/// `ATA_CORR`: transfer had a corrected error.
pub const ATA_CORR: u16 = 0x0040;
/// `ATA_LBA48`: transfer uses 48-bit LBA addressing.
pub const ATA_LBA48: u16 = 0x0080;

/// `NOERROR`: there was no error (`r_error` invalid).
pub const NOERROR: i32 = 0;
/// `ERROR`: check `r_error`.
pub const ERROR: i32 = 1;
/// `ERR_DF`: drive fault.
pub const ERR_DF: i32 = 2;
/// `ERR_DMA`: DMA error.
pub const ERR_DMA: i32 = 3;
/// `TIMEOUT`: device timed out.
pub const TIMEOUT: i32 = 4;
/// `ERR_NODEV`: device bas been detached.
pub const ERR_NODEV: i32 = 5;

/// `WDF_LOADED`: parameters loaded.
///
/// XXX Nothing resets this yet, but disk change sensing will when ATA-4 is more fully
/// implemented.
pub const WDF_LOADED: i32 = 0x10;
/// `WDF_WAIT`: waiting for resources.
pub const WDF_WAIT: i32 = 0x20;
/// `WDF_LBA`: using LBA mode.
pub const WDF_LBA: i32 = 0x40;
/// `WDF_LBA48`: using 48-bit LBA mode.
pub const WDF_LBA48: i32 = 0x80;

/* drive states stored in ata_drive_datas */

/// `RECAL`.
pub const RECAL: u8 = 0;
/// `RECAL_WAIT`.
pub const RECAL_WAIT: u8 = 1;
/// `PIOMODE`.
pub const PIOMODE: u8 = 2;
/// `PIOMODE_WAIT`.
pub const PIOMODE_WAIT: u8 = 3;
/// `DMAMODE`.
pub const DMAMODE: u8 = 4;
/// `DMAMODE_WAIT`.
pub const DMAMODE_WAIT: u8 = 5;
/// `GEOMETRY`.
pub const GEOMETRY: u8 = 6;
/// `GEOMETRY_WAIT`.
pub const GEOMETRY_WAIT: u8 = 7;
/// `MULTIMODE`.
pub const MULTIMODE: u8 = 8;
/// `MULTIMODE_WAIT`.
pub const MULTIMODE_WAIT: u8 = 9;
/// `READY`.
pub const READY: u8 = 10;

/// `struct ata_bio`: params needed by the controller to perform an ATA bio.
pub struct AtaBio {
    /// `flags`: cmd flags (`ATA_*`; `volatile` in the C).
    pub flags: Cell<u16>,
    /// `multi`: number of blocks to transfer in multi-mode.
    pub multi: Cell<i32>,
    /// `lp`: pointer to drive's label info.
    pub lp: Cell<Option<NonNull<Disklabel>>>,
    /// `blkno`: block addr.
    pub blkno: Cell<Daddr>,
    /// `blkdone`: number of blks transferred.
    pub blkdone: Cell<Daddr>,
    /// `nblks`: number of block currently transferring.
    pub nblks: Cell<Daddr>,
    /// `nbytes`: number of bytes currently transferring.
    pub nbytes: Cell<i32>,
    /// `bcount`: total number of bytes.
    pub bcount: Cell<i64>,
    /// `databuf`: data buffer address.
    pub databuf: Cell<*mut u8>,
    /// `error` (`volatile` in the C): `NOERROR`, `ERROR`, ...
    pub error: Cell<i32>,
    /// `r_error`: copy of error register.
    pub r_error: Cell<u8>,
    /// `wd`.
    pub wd: Cell<Option<NonNull<WdSoftc>>>,
}

impl AtaBio {
    /// All zero, as in an `M_ZERO` softc.
    pub const fn new() -> Self {
        Self {
            flags: Cell::new(0),
            multi: Cell::new(0),
            lp: Cell::new(None),
            blkno: Cell::new(0),
            blkdone: Cell::new(0),
            nblks: Cell::new(0),
            nbytes: Cell::new(0),
            bcount: Cell::new(0),
            databuf: Cell::new(ptr::null_mut()),
            error: Cell::new(0),
            r_error: Cell::new(0),
            wd: Cell::new(None),
        }
    }

    /// `ata_bio->flags & f`.
    pub fn isset(&self, f: u16) -> bool {
        self.flags.get() & f != 0
    }

    /// `ata_bio->flags |= f`.
    pub fn set(&self, f: u16) {
        self.flags.set(self.flags.get() | f);
    }

    /// `ata_bio->lp`, read in place. Panics where the C would dereference NULL (a transfer
    /// before `wdattach` attached the disk).
    pub fn label<R>(&self, f: impl FnOnce(&Disklabel) -> R) -> R {
        match self.lp.get() {
            // SAFETY: `lp` is the disk's in-core label (`wdattach` sets it once the disk is
            // attached), which lives until `disk_detach`; the driver writes it only with no
            // transfer in flight (`wdopen` with nothing open, `DIOCSDINFO` under the disk
            // lock, as the C).
            Some(lp) => f(unsafe { lp.as_ref() }),
            None => panic(format_args!("ata_bio: no disklabel")),
        }
    }
}

impl Default for AtaBio {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct wd_softc`.
#[repr(C)]
pub struct WdSoftc {
    /* General disk infos */
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_dk`.
    pub sc_dk: Disk,
    /// `sc_bufq`.
    pub sc_bufq: Bufq,

    /* IDE disk soft states */
    /// `sc_wdc_bio`: current transfer.
    pub sc_wdc_bio: AtaBio,
    /// `sc_bp`: buf being transferred.
    pub sc_bp: Cell<Option<&'static Buf>>,
    /// `drvp`: our controller's infos.
    pub drvp: Cell<Option<&'static AtaDriveDatas>>,
    /// `openings`.
    pub openings: Cell<i32>,
    /// `sc_params`: drive characteristics found.
    sc_params: UnsafeCell<Ataparams>,
    /// `sc_flags` (`WDF_*`).
    pub sc_flags: Cell<i32>,

    /// `sc_capacity`.
    pub sc_capacity: Cell<u64>,
    /// `cyl`: actual drive parameters.
    pub cyl: Cell<i32>,
    /// `heads`.
    pub heads: Cell<i32>,
    /// `sectors`.
    pub sectors: Cell<i32>,
    /// `retries`: number of xfer retry.
    pub retries: Cell<i32>,
    /// `sc_restart_timeout`.
    pub sc_restart_timeout: Timeout,
}

impl WdSoftc {
    /// `wd->drvp`, set by `wdattach`. Panics where the C would dereference NULL.
    pub fn drvp(&self) -> &'static AtaDriveDatas {
        match self.drvp.get() {
            Some(drvp) => drvp,
            None => panic(format_args!("{}: no drive data", self.sc_dev.xname())),
        }
    }

    /// `&wd->sc_params`, read in place.
    pub fn params(&self) -> &Ataparams {
        // SAFETY: `sc_params` is only written through `params_mut`, whose callers keep no
        // other reference live (its contract).
        unsafe { &*self.sc_params.get() }
    }

    /// `&wd->sc_params`, writable (`wd_get_params(wd, flags, &wd->sc_params)`).
    ///
    /// # Safety
    ///
    /// The caller is the disk's driver, serialised against every other user of the block
    /// (attach, the disk lock in `wdopen`, resume), and keeps no other reference to it while
    /// the returned one is live.
    #[allow(clippy::mut_from_ref)] // the C's struct member, shared by pointer
    pub unsafe fn params_mut(&self) -> &mut Ataparams {
        // SAFETY: the caller's contract.
        unsafe { &mut *self.sc_params.get() }
    }

    /// `wd->sc_flags & f`.
    pub fn isset(&self, f: i32) -> bool {
        self.sc_flags.get() & f != 0
    }

    /// `wd->sc_flags |= f`.
    pub fn set(&self, f: i32) {
        self.sc_flags.set(self.sc_flags.get() | f);
    }
}

// SAFETY: `#[repr(C)]` with the device first; the disk, the buffer queue and the timeout are
// all-zero valid (`sys/disk.rs`, `sys/buf.rs`, `sys/timeout.rs`), and the other members are
// `Cell`s of integers, raw pointers, `Option`s of references and `NonNull`s, and the
// IDENTIFY block, all valid as all zero bits.
unsafe impl Softc for WdSoftc {}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bio_flags() {
        let b = AtaBio::new();
        b.set(ATA_READ | ATA_LBA);
        assert!(b.isset(ATA_LBA));
        assert!(!b.isset(ATA_LBA48));
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ata/wdvar.h");
        let names = crate::reftest::assert_defines!(defs;
            ATA_NOSLEEP, ATA_POLL, ATA_ITSDONE, ATA_SINGLE, ATA_LBA, ATA_READ, ATA_CORR,
            ATA_LBA48, NOERROR, ERROR, ERR_DF, ERR_DMA, TIMEOUT, ERR_NODEV, WDF_LOADED,
            WDF_WAIT, WDF_LBA, WDF_LBA48, RECAL, RECAL_WAIT, PIOMODE, PIOMODE_WAIT, DMAMODE,
            DMAMODE_WAIT, GEOMETRY, GEOMETRY_WAIT, MULTIMODE, MULTIMODE_WAIT, READY,
        );
        for prefix in [
            "ATA_", "ERR", "WDF_", "RECAL", "PIOMODE", "DMAMODE", "GEOMETRY",
        ] {
            crate::reftest::assert_complete(&defs, prefix, &names);
        }
    }
}
/* </TESTS> */
