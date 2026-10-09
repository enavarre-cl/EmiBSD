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
 * Copyright (c) 1993, 1994, 1995 Charles Hannum.
 * Copyright (c) 1990 The Regents of the University of California.
 * All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * Don Ahn.
 *
 * Portions Copyright (c) 1993, 1994 by
 *  jc@irbs.UUCP (John Capo)
 *  vak@zebub.msk.su (Serge Vakulenko)
 *  ache@astral.msk.su (Andrew A. Chernov)
 *  joerg_wunsch@uriah.sax.de (Joerg Wunsch)
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 */
/* </LICENSES> */

/* <CODE> */
//! The goo that binds the floppy controller to its devices: `<dev/isa/fdlink.h>`.
//!
//! Upstream: sys/dev/isa/fdlink.h @ 3ce1f3f79392
//!
//! fdc(4)'s softc, its controller states, the drive queue its fd(4) children share, and the
//! attach arguments `fdcattach` hands each drive. The functions it declares are `fdc.c`'s
//! (`fdcresult`, `out_fdc`, `fdcstart`, `fdcstatus`, `fdcpseudointr`) and `fd.c`'s
//! (`fd_nvtotype`). Like them, it exists where cfg `machine_x86` is set: `struct fd_type`
//! is `<machine/ioctl_fd.h>`'s, an x86 header (`docs/ARCHITECTURE.md`, `machine_x86`).
//!
//! ## Deviations
//! - `enum fdc_state` and `enum fdc_type` are `i32` constants: `fdtimeout` steps the state
//!   with `sc_state++`.
//! - `sc_link`'s union of `struct fdc_fdlink` and `struct fdc_ftlink` is the disk link
//!   alone: `struct ft_softc` (the QIC-117 tape driver) exists in no file of the tree, and
//!   nothing reads `ftlink`.
//! - The softc's members are `Cell`s: fdc.c and fd.c change them at `splbio` from the
//!   strategy routine, the interrupt handler and the timeouts, as the C does. `sc_fd[]`
//!   holds `&'static FdSoftc`s (softcs live as long as the kernel).

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;

use crate::dev::isa::fd::{FdDrivechain, FdSoftc};
use crate::dev::isa::isavar::Isadev;
use crate::kern::subr_prf::panic;
use crate::machine::bus::{BusSpaceHandle, BusSpaceTag};
use crate::machine::x86::FdType;
use crate::sys::device::{Device, Softc};
use crate::sys::queue::TailqHead;
use crate::sys::timeout::Timeout;

/// `enum fdc_state`.
pub type FdcState = i32;
/// `DEVIDLE`.
pub const DEVIDLE: FdcState = 0;
/// `MOTORWAIT`.
pub const MOTORWAIT: FdcState = 1;
/// `DOSEEK`.
pub const DOSEEK: FdcState = 2;
/// `SEEKWAIT`.
pub const SEEKWAIT: FdcState = 3;
/// `SEEKTIMEDOUT`.
pub const SEEKTIMEDOUT: FdcState = 4;
/// `SEEKCOMPLETE`.
pub const SEEKCOMPLETE: FdcState = 5;
/// `DOIO`.
pub const DOIO: FdcState = 6;
/// `IOCOMPLETE`.
pub const IOCOMPLETE: FdcState = 7;
/// `IOTIMEDOUT`.
pub const IOTIMEDOUT: FdcState = 8;
/// `DORESET`.
pub const DORESET: FdcState = 9;
/// `RESETCOMPLETE`.
pub const RESETCOMPLETE: FdcState = 10;
/// `RESETTIMEDOUT`.
pub const RESETTIMEDOUT: FdcState = 11;
/// `DORECAL`.
pub const DORECAL: FdcState = 12;
/// `RECALWAIT`.
pub const RECALWAIT: FdcState = 13;
/// `RECALTIMEDOUT`.
pub const RECALTIMEDOUT: FdcState = 14;
/// `RECALCOMPLETE`.
pub const RECALCOMPLETE: FdcState = 15;

/// `enum fdc_type`.
pub type FdcType = i32;
/// `FDC_TYPE_TAPE`.
pub const FDC_TYPE_TAPE: FdcType = 0;
/// `FDC_TYPE_DISK`.
pub const FDC_TYPE_DISK: FdcType = 1;

/// `struct fdc_fdlink`: software state, per controller.
pub struct FdcFdlink {
    /// `sc_fd[4]`: pointers to children.
    pub sc_fd: [Cell<Option<&'static FdSoftc>>; 4],
    /// `sc_drives`: the drives with a transfer queued, the active one first.
    pub sc_drives: TailqHead<FdDrivechain>,
}

/// `struct fdc_softc`.
#[repr(C)]
pub struct FdcSoftc {
    /// `sc_dev`: boilerplate.
    pub sc_dev: Device,
    /// `sc_id`.
    pub sc_id: Isadev,
    /// `sc_ih`.
    pub sc_ih: Cell<Option<NonNull<c_void>>>,

    /// `sc_iot`: ISA chipset identifier.
    pub sc_iot: Cell<Option<BusSpaceTag>>,
    /// `sc_ioh`: ISA io handle.
    pub sc_ioh: Cell<Option<BusSpaceHandle>>,
    /// `sc_ioh_ctl`: ISA io handle.
    pub sc_ioh_ctl: Cell<Option<BusSpaceHandle>>,

    /// `sc_drq`.
    pub sc_drq: Cell<i32>,

    /// `sc_type[4]`: type of device.
    pub sc_type: [Cell<FdcType>; 4],
    /// `sc_link.fdlink`.
    pub sc_link: FdcFdlink,
    /// `sc_state`.
    pub sc_state: Cell<FdcState>,
    /// `sc_errors`: number of retries so far.
    pub sc_errors: Cell<i32>,
    /// `fdcpseudointr_to`.
    pub fdcpseudointr_to: Timeout,
    /// `sc_status[7]`: copy of registers.
    pub sc_status: [Cell<u8>; 7],
}

impl FdcSoftc {
    /// `fdc->sc_iot`, set by `fdcattach`.
    pub fn iot(&self) -> BusSpaceTag {
        match self.sc_iot.get() {
            Some(t) => t,
            None => panic(format_args!("{}: no tag", self.sc_dev.xname())),
        }
    }

    /// `fdc->sc_ioh`, set by `fdcattach`.
    pub fn ioh(&self) -> BusSpaceHandle {
        match self.sc_ioh.get() {
            Some(h) => h,
            None => panic(format_args!("{}: not mapped", self.sc_dev.xname())),
        }
    }

    /// `fdc->sc_ioh_ctl`, set by `fdcattach`.
    pub fn ioh_ctl(&self) -> BusSpaceHandle {
        match self.sc_ioh_ctl.get() {
            Some(h) => h,
            None => panic(format_args!("{}: not mapped", self.sc_dev.xname())),
        }
    }
}

// SAFETY: `repr(C)` with the device first; all-zero is a valid value of every member (`None`
// cells, null list links, a zeroed timeout as `timeout_set` expects, zero counters and
// states: `DEVIDLE`, `FDC_TYPE_TAPE`).
unsafe impl Softc for FdcSoftc {}

/// `struct fdc_attach_args`: arguments passed between fdcattach and f[dt]probe.
pub struct FdcAttachArgs {
    /// `fa_drive`.
    pub fa_drive: i32,
    /// `fa_flags`.
    pub fa_flags: i32,
    /// `fa_type`: tape drive type.
    pub fa_type: i32,
    /// `fa_deftype`.
    pub fa_deftype: Option<&'static FdType>,
}
/* </CODE> */
