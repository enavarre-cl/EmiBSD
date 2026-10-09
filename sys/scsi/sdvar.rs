/* <LICENSES> */
/*-
 * Copyright (c) 1998 The NetBSD Foundation, Inc.
 * All rights reserved.
 *
 * This code is derived from software contributed to The NetBSD Foundation
 * by Charles M. Hannum.
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

/*
 * Originally written by Julian Elischer (julian@dialix.oz.au)
 * for TRW Financial Systems for use under the MACH(2.5) operating system.
 *
 * TRW Financial Systems, in accordance with their agreement with Carnegie
 * Mellon University, makes this software available to CMU to distribute
 * or use in any manner that they see fit as long as this message is kept with
 * the software. For this reason TFS also grants any other persons or
 * organisations permission to use or modify this software.
 *
 * TFS supplies this software to be publicly redistributed
 * on the understanding that TFS is not responsible for the correct
 * functioning of this software in any circumstances.
 *
 * Ported to run under 386BSD by Julian Elischer (julian@dialix.oz.au) Sept 1992
 */
/* </LICENSES> */

/* <CODE> */
//! The softc of sd(4), the SCSI disk driver (`sd.rs`).
//!
//! Upstream: sys/scsi/sdvar.h @ 3ce1f3f79392
//!
//! `struct sd_softc` holds the generic device and disk, the queue of buffers waiting for the
//! device, the link to the target, and the geometry read from it (`struct disk_parms`).
//!
//! ## Deviations
//! - The members the driver changes after attach (`flags`, `sc_link`, `params`) are `Cell`s:
//!   the softc is shared by reference between the device switch entry points and the
//!   transfer completions, as the C shares it by pointer. `struct disk_parms` is a plain
//!   `Copy` structure read and written whole through its `Cell` (`sd_get_parms` already
//!   works on a copy).
//! - `ISSET`/`SET`/`CLR` on `flags` are the methods `isset`/`set`/`clr`, as on `struct buf`.

use core::cell::Cell;

use crate::kern::subr_prf::panic;
use crate::scsi::scsiconf::{ScsiLink, ScsiXshandler};
use crate::sys::buf::Bufq;
use crate::sys::device::{Device, Softc};
use crate::sys::disk::Disk;

/// `SDF_THIN`: disk is thin provisioned.
pub const SDF_THIN: i32 = 0x01;
/// `SDF_DIRTY`: disk is dirty; needs cache flush.
pub const SDF_DIRTY: i32 = 0x20;
/// `SDF_DYING`: dying, when deactivated.
pub const SDF_DYING: i32 = 0x40;

/// `struct disk_parms`: what `sd_get_parms` learnt of the disk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiskParms {
    /// `heads`: number of heads.
    pub heads: u32,
    /// `cyls`: number of cylinders.
    pub cyls: u32,
    /// `sectors`: number of sectors/track.
    pub sectors: u32,
    /// `secsize`: number of bytes/sector.
    pub secsize: u32,
    /// `disksize`: total number sectors.
    pub disksize: u64,
    /// `unmap_sectors`: maximum sectors/unmap.
    pub unmap_sectors: u32,
    /// `unmap_descs`: maximum descriptors/unmap.
    pub unmap_descs: u32,
}

impl DiskParms {
    /// All zero, as in an `M_ZERO` softc.
    pub const fn new() -> Self {
        Self {
            heads: 0,
            cyls: 0,
            sectors: 0,
            secsize: 0,
            disksize: 0,
            unmap_sectors: 0,
            unmap_descs: 0,
        }
    }
}

/// `struct sd_softc`.
#[repr(C)]
pub struct SdSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_dk`.
    pub sc_dk: Disk,
    /// `sc_bufq`: the buffers `sdstrategy` queued for `sdstart`.
    pub sc_bufq: Bufq,
    /// `flags`: `SDF_*`.
    pub flags: Cell<i32>,
    /// `sc_link`: contains our targ, lun, etc.; set by `sdattach`.
    pub sc_link: Cell<Option<&'static ScsiLink>>,
    /// `params`.
    pub params: Cell<DiskParms>,
    /// `sc_xsh`: runs `sdstart` when the link has an opening.
    pub sc_xsh: ScsiXshandler,
}

impl SdSoftc {
    /// `sc->sc_link`: the link `sdattach` stored.
    ///
    /// Panics before `sdattach`, where the C would dereference NULL.
    pub fn link(&self) -> &'static ScsiLink {
        match self.sc_link.get() {
            Some(link) => link,
            None => panic(format_args!("{}: no scsi_link", self.sc_dev.xname())),
        }
    }

    /// `ISSET(sc->flags, f)`.
    pub fn isset(&self, f: i32) -> bool {
        self.flags.get() & f != 0
    }

    /// `SET(sc->flags, f)`.
    pub fn set(&self, f: i32) {
        self.flags.set(self.flags.get() | f);
    }

    /// `CLR(sc->flags, f)`.
    pub fn clr(&self, f: i32) {
        self.flags.set(self.flags.get() & !f);
    }
}

// SAFETY: `#[repr(C)]` with the device first; the disk, the buffer queue and the transfer
// handler are all-zero valid (`sys/disk.rs`; `sys/buf.rs`: integers, null pointers, `None`s
// and a free mutex; `scsiconf.rs`), and the other members are `Cell`s of an integer, an
// `Option` of a reference or a structure of integers.
unsafe impl Softc for SdSoftc {}
/* </CODE> */
