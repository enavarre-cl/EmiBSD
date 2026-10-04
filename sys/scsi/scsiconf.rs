/*	$OpenBSD: scsiconf.h,v 1.202 2023/05/10 15:28:26 krw Exp $	*/
/*	$NetBSD: scsiconf.h,v 1.35 1997/04/02 02:29:38 mycroft Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1993, 1994, 1995 Charles Hannum.  All rights reserved.
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
 *	This product includes software developed by Charles Hannum.
 * 4. The name of the author may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
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

/*
 * Originally written by Julian Elischer (julian@tfs.com)
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
 * Ported to run under 386BSD by Julian Elischer (julian@tfs.com) Sept 1992
 */
/* </LICENSES> */

//! `<scsi/scsiconf.h>`: the SCSI midlayer's structures, which connect an adapter (host bus
//! adapter) driver, the `scsibus` above it, and the device drivers (`sd`, `cd`, `st`, ...)
//! attached to its LUNs: the adapter's entry points (`struct scsi_adapter`), one link per
//! LUN (`struct scsi_link`), one transfer per command (`struct scsi_xfer`), the I/O pools and
//! their handlers that meter the adapter's openings, the bus's softc and attach arguments,
//! and the big-endian helpers (`_lto4b`, `_4btol`, ...) every CDB uses.
//!
//! Upstream: sys/scsi/scsiconf.h @ 3ce1f3f79392
//!
//! The functions this header declares live in `scsi_base.rs` (the transfer and pool
//! machinery, `scsi_xs_get`, `scsi_xs_exec`, `scsi_done`, ...) and, for `scsiconf.c`
//! (probe, attach, detach, `scsiprint`, `scsi_inqmatch`), here once that file is ported.
//!
//! ## Ownership
//! - A [`ScsiLink`], its [`ScsiIopool`] when the link owns one, a [`ScsibusSoftc`] and an
//!   adapter's pool are long-lived kernel objects reached as `&'static`: `scsiconf.c`
//!   allocates a link at probe and frees it at detach only after `scsi_link_shutdown` has
//!   waited for every transfer, and a softc lives until its device detaches. Nothing may use
//!   them after that; this is the C's contract, as for vnodes and softcs.
//! - A [`ScsiXfer`] is a `scsi_xfer_pool` item, reached as `&'static ScsiXfer` from
//!   `scsi_xs_get` until `scsi_xs_put` gives it back (`docs/C_TO_RUST.md`, the `struct buf *`
//!   row). Between `scsi_xs_exec` and `scsi_done` the adapter owns it; after `scsi_done` its
//!   `done` callback (or `scsi_xs_sync`) does.
//! - `xs->data` is a raw pointer with its length, private to [`ScsiXfer`]: the issuer sets
//!   both with the `unsafe` [`ScsiXfer::set_data`] (the buffer outlives the transfer and is
//!   not touched by anyone else meanwhile), so that the adapter's and `scsi_base`'s reads and
//!   writes through it are sound.
//!
//! ## Deviations
//! - Every member the C changes through a shared pointer is a `Cell`, as for `struct buf`;
//!   the iopool's mutex (`pool->mtx`) protects the run queues, `running` and the link's
//!   `pending` as in C, taken with `mtx_enter`/`mtx_leave`.
//! - `link->device_softc` is `Cell<Option<NonNull<Device>>>`: every device driver stores its
//!   softc, which begins with `struct device`, and `sc_print_addr` reads it as one; the
//!   driver gets its softc back with `Device::softc`.
//! - `link->interpret_sense` is a `fn` that is never NULL: [`ScsiLink::new`] sets it to
//!   `scsi_interpret_sense`, as `scsi_probe_link` does.
//! - `struct scsi_iohandler` has a private `is_xsh` flag, set by the `scsi_xsh_*` functions:
//!   the C casts any handler on a pool's queue to `struct scsi_xshandler` in
//!   `scsi_link_shutdown` and tells the kinds apart by comparing the `handler` with
//!   `scsi_xs_get_done`/`scsi_xsh_ioh`, but Rust does not promise distinct addresses for
//!   functions with identical bodies (`scsi_io_get_done` and `scsi_xs_get_done`), so the
//!   cast is guarded by the flag.
//! - The `void *` I/O resource (`io_get`'s result, `xs->io`) is [`ScsiIo`], a `NonNull`;
//!   NULL is `None`. `io_get`/`io_put` and the handler of an [`ScsiIohandler`] are `unsafe
//!   fn`s over the pool's `iocookie`/the handler's `cookie`, which `scsi_iopool_init` and
//!   `scsi_ioh_set` pair with them (`docs/C_TO_RUST.md`, the `bufq_impl` row).
//! - `struct devid` keeps its identifier bytes after the header, as in C; [`Devid::id`] and
//!   [`devid_cmp`] (`DEVID_CMP`) are `unsafe` for that reason. `devid_alloc`, `devid_copy`
//!   and `devid_free` are `scsiconf.c`'s.
//! - `struct scsi_inquiry_pattern`'s strings are byte slices without the NUL.
//! - `_lto2b` ... `_8btol` take and return slices (`&mut [u8]`/`&[u8]`) instead of `u_int8_t
//!   *`; they index the first 2..8 bytes (a shorter slice panics).
//! - `SID_ANSII_REV(x)` and `SID_RESPONSE_FORMAT(x)` are `const fn`s ([`sid_ansii_rev`],
//!   [`sid_response_format`]).
//! - `scsi_autoconf`, `scsiprint`, `scsi_inqmatch` and the probe, detach, activate and
//!   `scsi_get_link` functions belong to `scsiconf.c` (not ported yet). The two that
//!   `scsi_base.c` calls, [`scsi_probe`] and [`scsi_detach`], are visible stubs
//!   (`unported!`) until it is.
//! - The prototypes of the header are not repeated: Rust needs none.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;

use crate::kern::subr_prf::panic;
use crate::queue_adapter;
use crate::scsi::scsi_all::{
    SID_ANSII, SID_RESPONSE_DATA_FMT, ScsiGeneric, ScsiInquiryData, ScsiSenseData, ScsiWire,
    wire_mut, wire_ref,
};
use crate::scsi::scsi_base::scsi_interpret_sense;
use crate::sys::buf::Buf;
use crate::sys::device::{Device, Softc};
use crate::sys::errno::Errno;
use crate::sys::mutex::Mutex;
use crate::sys::queue::{SimpleqEntry, SimpleqHead, SlistEntry, SlistHead, TailqEntry, TailqHead};
use crate::sys::timeout::Timeout;
use crate::unported;

/// `DEVID_NONE`: no device identifier.
pub const DEVID_NONE: u8 = 0;
/// `DEVID_NAA`: an NAA identifier.
pub const DEVID_NAA: u8 = 1;
/// `DEVID_EUI`: an EUI-64 identifier.
pub const DEVID_EUI: u8 = 2;
/// `DEVID_T10`: a T10 vendor identifier.
pub const DEVID_T10: u8 = 3;
/// `DEVID_SERIAL`: a serial number.
pub const DEVID_SERIAL: u8 = 4;
/// `DEVID_WWN`: a world wide name.
pub const DEVID_WWN: u8 = 5;

/// `DEVID_F_PRINT` (`d_flags`): the identifier is printable.
pub const DEVID_F_PRINT: u8 = 1 << 0;

/// `SDEV_S_DYING` (`scsi_link.state`): the link is being detached.
pub const SDEV_S_DYING: u32 = 1 << 1;

/// `SDEV_REMOVABLE` (`scsi_link.flags`): media is removable.
pub const SDEV_REMOVABLE: u16 = 0x0001;
/// `SDEV_MEDIA_LOADED`: device figures are still valid.
pub const SDEV_MEDIA_LOADED: u16 = 0x0002;
/// `SDEV_READONLY`: device is read-only.
pub const SDEV_READONLY: u16 = 0x0004;
/// `SDEV_OPEN`: at least 1 open session.
pub const SDEV_OPEN: u16 = 0x0008;
/// `SDEV_DBX`: debugging flags (`scsi_debug.h`).
pub const SDEV_DBX: u16 = 0x00f0;
/// `SDEV_EJECTING`: eject on device close.
pub const SDEV_EJECTING: u16 = 0x0100;
/// `SDEV_ATAPI`: device is ATAPI.
pub const SDEV_ATAPI: u16 = 0x0200;
/// `SDEV_UMASS`: device is UMASS SCSI.
pub const SDEV_UMASS: u16 = 0x0400;
/// `SDEV_VIRTUAL`: device is virtualised on the HBA.
pub const SDEV_VIRTUAL: u16 = 0x0800;
/// `SDEV_OWN_IOPL`: the link's iopool is its own (made by `scsibus`).
pub const SDEV_OWN_IOPL: u16 = 0x1000;
/// `SDEV_UFI`: universal floppy interface.
pub const SDEV_UFI: u16 = 0x2000;

/// `SDEV_AUTOSAVE` (`scsi_link.quirks`): do implicit SAVEDATAPOINTER on disconnect.
pub const SDEV_AUTOSAVE: u16 = 0x0001;
/// `SDEV_NOSYNC`: does not grok SDTR.
pub const SDEV_NOSYNC: u16 = 0x0002;
/// `SDEV_NOWIDE`: does not grok WDTR.
pub const SDEV_NOWIDE: u16 = 0x0004;
/// `SDEV_NOTAGS`: lies about having tagged queueing.
pub const SDEV_NOTAGS: u16 = 0x0008;
/// `SDEV_NOSYNCCACHE`: no SYNCHRONIZE_CACHE.
pub const SDEV_NOSYNCCACHE: u16 = 0x0010;
/// `ADEV_NOSENSE`: no request sense (ATAPI).
pub const ADEV_NOSENSE: u16 = 0x0020;
/// `ADEV_LITTLETOC`: little-endian TOC (ATAPI).
pub const ADEV_LITTLETOC: u16 = 0x0040;
/// `ADEV_NOCAPACITY`: no READ CD CAPACITY (ATAPI).
pub const ADEV_NOCAPACITY: u16 = 0x0080;
/// `ADEV_NODOORLOCK`: can't lock door (ATAPI).
pub const ADEV_NODOORLOCK: u16 = 0x0100;

/// `SDEV_NO_ADAPTER_TARGET` (`saa_adapter_target`): the adapter is not a target on its bus.
pub const SDEV_NO_ADAPTER_TARGET: u16 = 0xffff;

/*
 * Per-request Flag values
 */
/// `SCSI_NOSLEEP`: don't sleep.
pub const SCSI_NOSLEEP: i32 = 0x00001;
/// `SCSI_POLL`: poll for completion.
pub const SCSI_POLL: i32 = 0x00002;
/// `SCSI_AUTOCONF`: shorthand for `SCSI_POLL | SCSI_NOSLEEP`.
pub const SCSI_AUTOCONF: i32 = 0x00003;
/// `ITSDONE`: the transfer is as done as it gets.
pub const ITSDONE: i32 = 0x00008;
/// `SCSI_SILENT`: don't announce NOT READY or MEDIA CHANGE.
pub const SCSI_SILENT: i32 = 0x00020;
/// `SCSI_IGNORE_NOT_READY`: ignore NOT READY.
pub const SCSI_IGNORE_NOT_READY: i32 = 0x00040;
/// `SCSI_IGNORE_MEDIA_CHANGE`: ignore MEDIA CHANGE.
pub const SCSI_IGNORE_MEDIA_CHANGE: i32 = 0x00080;
/// `SCSI_IGNORE_ILLEGAL_REQUEST`: ignore ILLEGAL REQUEST.
pub const SCSI_IGNORE_ILLEGAL_REQUEST: i32 = 0x00100;
/// `SCSI_RESET`: reset the device in question.
pub const SCSI_RESET: i32 = 0x00200;
/// `SCSI_DATA_IN`: expect data to come INTO memory.
pub const SCSI_DATA_IN: i32 = 0x00800;
/// `SCSI_DATA_OUT`: expect data to flow OUT of memory.
pub const SCSI_DATA_OUT: i32 = 0x01000;
/// `SCSI_TARGET`: this defines a TARGET mode op.
pub const SCSI_TARGET: i32 = 0x02000;
/// `SCSI_ESCAPE`: escape operation.
pub const SCSI_ESCAPE: i32 = 0x04000;
/// `SCSI_PRIVATE`: private to each HBA.
pub const SCSI_PRIVATE: i32 = 0xf0000;

/*
 * Escape op-codes.  This provides an extensible setup for operations
 * that are not scsi commands.  They are intended for modal operations.
 */
/// `SCSI_OP_TARGET`.
pub const SCSI_OP_TARGET: i32 = 0x0001;
/// `SCSI_OP_RESET`.
pub const SCSI_OP_RESET: i32 = 0x0002;
/// `SCSI_OP_BDINFO`.
pub const SCSI_OP_BDINFO: i32 = 0x0003;

/*
 * Error values an adapter driver may return
 */
/// `XS_NOERROR`: there is no error (sense is invalid).
pub const XS_NOERROR: i32 = 0;
/// `XS_SENSE`: check the returned sense for the error.
pub const XS_SENSE: i32 = 1;
/// `XS_DRIVER_STUFFUP`: driver failed to perform operation.
pub const XS_DRIVER_STUFFUP: i32 = 2;
/// `XS_SELTIMEOUT`: the device timed out (turned off?).
pub const XS_SELTIMEOUT: i32 = 3;
/// `XS_TIMEOUT`: the timeout reported was caught by software.
pub const XS_TIMEOUT: i32 = 4;
/// `XS_BUSY`: the device is busy, try again later.
pub const XS_BUSY: i32 = 5;
/// `XS_SHORTSENSE`: check the ATAPI sense for the error.
pub const XS_SHORTSENSE: i32 = 6;
/// `XS_RESET`: bus was reset; possible retry command.
pub const XS_RESET: i32 = 8;

/// `TEST_READY_RETRIES`: possible retries for `scsi_test_unit_ready()`.
pub const TEST_READY_RETRIES: i32 = 5;

/// `SCSI_RETRIES`: possible retries for most SCSI commands.
pub const SCSI_RETRIES: i32 = 4;

/// `SCSI_REV_0`: no conformance to any standard.
pub const SCSI_REV_0: u8 = 0x00;
/// `SCSI_REV_1`: (obsolete) SCSI-1 in olden times.
pub const SCSI_REV_1: u8 = 0x01;
/// `SCSI_REV_2`: (obsolete) SCSI-2 in olden times.
pub const SCSI_REV_2: u8 = 0x02;
/// `SCSI_REV_SPC`: ANSI INCITS 301-1997 (SPC).
pub const SCSI_REV_SPC: u8 = 0x03;
/// `SCSI_REV_SPC2`: ANSI INCITS 351-2001 (SPC-2).
pub const SCSI_REV_SPC2: u8 = 0x04;
/// `SCSI_REV_SPC3`: ANSI INCITS 408-2005 (SPC-3).
pub const SCSI_REV_SPC3: u8 = 0x05;
/// `SCSI_REV_SPC4`: ANSI INCITS 513-2015 (SPC-4).
pub const SCSI_REV_SPC4: u8 = 0x06;
/// `SCSI_REV_SPC5`: T10/BSR INCITS 503 (SPC-5).
pub const SCSI_REV_SPC5: u8 = 0x07;

/// `SCSI_IOPOOL_POISON`: the "opening" the default io allocator hands out.
pub const SCSI_IOPOOL_POISON: ScsiIo =
    NonNull::without_provenance(match core::num::NonZeroUsize::new(0x5c5) {
        Some(n) => n,
        None => unreachable!(),
    });

/// `struct devid`: a device identifier, the header of an allocation whose `d_len`
/// identifier bytes follow it.
#[repr(C)]
#[derive(Debug)]
pub struct Devid {
    /// `d_type`: `DEVID_*`.
    pub d_type: u8,
    /// `d_flags`: `DEVID_F_PRINT`.
    pub d_flags: u8,
    /// `d_refcount`: references (`devid_copy`, `devid_free`).
    pub d_refcount: Cell<u8>,
    /// `d_len`: length of the identifier after the header.
    pub d_len: u8,
}

impl Devid {
    /// The identifier bytes after the header (`(u_int8_t *)(d + 1)`).
    ///
    /// # Safety
    ///
    /// `self` is the header of an allocation made by `devid_alloc`, which holds `d_len`
    /// identifier bytes right after it.
    pub unsafe fn id(&self) -> &[u8] {
        // SAFETY: the caller's contract: `d_len` initialised bytes follow the header, in the
        // same allocation, which lives as long as `self` is borrowed.
        unsafe {
            slice::from_raw_parts(
                ptr::from_ref(self).add(1).cast::<u8>(),
                usize::from(self.d_len),
            )
        }
    }
}

/// `struct scsi_adapter`: the entry points the device drivers and the midlayer call in the
/// adapter driver. Each adapter type has one, statically allocated.
#[derive(Clone, Copy)]
pub struct ScsiAdapter {
    /// `scsi_cmd`: starts a transfer; the adapter calls `scsi_done(xs)` when it is over
    /// (before returning when `SCSI_POLL` is set).
    pub scsi_cmd: fn(xs: &'static ScsiXfer),
    /// `dev_minphys`: trims a transfer to what the adapter can do.
    pub dev_minphys: Option<fn(bp: &'static Buf, link: &'static ScsiLink)>,
    /// `dev_probe`: asks the adapter whether a link should be probed.
    pub dev_probe: Option<ScsiDevProbeFn>,
    /// `dev_free`: the link is going away.
    pub dev_free: Option<fn(link: &'static ScsiLink)>,
    /// `ioctl`: an adapter-specific ioctl.
    pub ioctl: Option<ScsiAdapterIoctlFn>,
}

/// `int (*dev_probe)(struct scsi_link *)` of [`ScsiAdapter`].
pub type ScsiDevProbeFn = fn(link: &'static ScsiLink) -> Result<(), Errno>;

/// `int (*interpret_sense)(struct scsi_xfer *)` of [`ScsiLink`]: the errno for a transfer
/// that ended with sense data (`Err(ERESTART)`: retry).
pub type ScsiInterpretSenseFn = fn(xs: &'static ScsiXfer) -> Result<(), Errno>;

/// `int (*ioctl)(struct scsi_link *, u_long, caddr_t, int)` of [`ScsiAdapter`].
///
/// # Safety
///
/// `data` is an aligned kernel copy of the command's argument structure (`sys_ioctl`'s
/// contract, `docs/C_TO_RUST.md`).
pub type ScsiAdapterIoctlFn =
    unsafe fn(link: &'static ScsiLink, cmd: u64, data: *mut u8, flag: i32) -> Result<(), Errno>;

/// An I/O resource ("opening") of an adapter: the `void *` `io_get` returns and `xs->io`
/// holds (an adapter's request slot, or [`SCSI_IOPOOL_POISON`]).
pub type ScsiIo = NonNull<c_void>;

/// `void *(*io_get)(void *)`: reserves everything needed to send one transfer; `None` when
/// nothing is free.
///
/// # Safety
///
/// `iocookie` is the cookie `scsi_iopool_init` paired with this function.
pub type ScsiIoGetFn = unsafe fn(iocookie: *mut c_void) -> Option<ScsiIo>;

/// `void (*io_put)(void *, void *)`: gives an opening back.
///
/// # Safety
///
/// `iocookie` is the cookie `scsi_iopool_init` paired with this function, and `io` came from
/// the paired `io_get`.
pub type ScsiIoPutFn = unsafe fn(iocookie: *mut c_void, io: ScsiIo);

/// `void (*handler)(void *, void *)` of an I/O handler: called with an opening, or with
/// `None` when the pool or the link is shut down under it.
///
/// # Safety
///
/// `cookie` is the cookie `scsi_ioh_set` paired with this function.
pub type ScsiIohFn = unsafe fn(cookie: *mut c_void, io: Option<ScsiIo>);

/// `struct scsi_iohandler`: a request for an opening, queued on a pool (or, inside an
/// [`ScsiXshandler`], on a link) until one is free.
pub struct ScsiIohandler {
    /// `q_entry`: the link in a run queue. Protected by: the pool's `mtx`.
    pub q_entry: TailqEntry<ScsiIohandler>,
    /// `q_state`: `RUNQ_IDLE`, `RUNQ_LINKQ` or `RUNQ_POOLQ`. Protected by: the pool's `mtx`.
    pub q_state: Cell<u32>,
    /// `pool`: the pool the openings come from.
    pub pool: Cell<Option<&'static ScsiIopool>>,
    /// `handler`: called with the opening.
    pub handler: Cell<Option<ScsiIohFn>>,
    /// `cookie`: the handler's argument.
    pub cookie: Cell<*mut c_void>,
    /// Whether this handler is the `ioh` of an [`ScsiXshandler`] (see the module's
    /// deviations).
    pub(crate) is_xsh: Cell<bool>,
}

impl ScsiIohandler {
    /// An idle handler for no pool (zeroed, as a softc member starts).
    pub const fn new() -> Self {
        Self {
            q_entry: TailqEntry::new(),
            q_state: Cell::new(0),
            pool: Cell::new(None),
            handler: Cell::new(None),
            cookie: Cell::new(ptr::null_mut()),
            is_xsh: Cell::new(false),
        }
    }
}

impl Default for ScsiIohandler {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `TAILQ_HEAD(scsi_runq, scsi_iohandler)`: the handlers waiting on a pool or a link,
    /// through `q_entry`.
    pub ScsiRunqEntries: ScsiIohandler, q_entry => TailqEntry<ScsiIohandler>
);

/// `struct scsi_runq`.
pub type ScsiRunq = TailqHead<ScsiRunqEntries>;

/// `struct scsi_iopool`: an adapter's openings and the handlers waiting for one.
pub struct ScsiIopool {
    /// `iocookie`: the argument of `io_get` and `io_put`.
    pub iocookie: Cell<*mut c_void>,
    /// `io_get`: gets an opening. It must reserve all resources necessary to send the
    /// transfer to the device; they stay reserved for the opening's lifetime, as an opening
    /// may be reused without going through `io_put` first.
    pub io_get: Cell<Option<ScsiIoGetFn>>,
    /// `io_put`: gives an opening back.
    pub io_put: Cell<Option<ScsiIoPutFn>>,
    /// `queue`: the run queue. Protected by: `mtx`.
    pub queue: ScsiRunq,
    /// `running`: the run queue semaphore. Protected by: `mtx`.
    pub running: Cell<u32>,
    /// `mtx`: protects the run queue and its semaphore (and the links' `queue`,
    /// `running` and `pending`).
    pub mtx: Mutex,
}

impl ScsiIopool {
    /// An uninitialised pool (zeroed, as a softc member starts); `scsi_iopool_init` sets it
    /// up.
    pub const fn new() -> Self {
        Self {
            iocookie: Cell::new(ptr::null_mut()),
            io_get: Cell::new(None),
            io_put: Cell::new(None),
            queue: ScsiRunq::new(),
            running: Cell::new(0),
            mtx: Mutex::new(0),
        }
    }
}

impl Default for ScsiIopool {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct scsi_xshandler`: a device driver's request for a transfer, queued on its link
/// until the link has an opening, then on the pool until the adapter has one; the handler
/// then gets a ready [`ScsiXfer`].
#[repr(C)]
pub struct ScsiXshandler {
    /// `ioh`: must be first.
    pub ioh: ScsiIohandler,
    /// `link`: the link the transfer is for.
    pub link: Cell<Option<&'static ScsiLink>>,
    /// `handler`: called with the transfer.
    pub handler: Cell<Option<fn(xs: &'static ScsiXfer)>>,
}

impl ScsiXshandler {
    /// An idle handler (zeroed, as a softc member starts); `scsi_xsh_set` sets it up.
    pub const fn new() -> Self {
        Self {
            ioh: ScsiIohandler::new(),
            link: Cell::new(None),
            handler: Cell::new(None),
        }
    }
}

impl Default for ScsiXshandler {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct scsi_link`: the connection between an adapter driver and a device driver for
/// one LUN, used by each to call services of the other and by the midlayer for both.
pub struct ScsiLink {
    /// `bus_list`: the link in the bus's `sc_link_list`.
    pub bus_list: SlistEntry<ScsiLink>,
    /// `state`: `SDEV_S_DYING`.
    pub state: Cell<u32>,
    /// `target`: target of this device.
    pub target: Cell<u16>,
    /// `lun`: LUN of this device.
    pub lun: Cell<u16>,
    /// `openings`: available operations per LUN.
    pub openings: Cell<u16>,
    /// `port_wwn`: world wide name of the port.
    pub port_wwn: Cell<u64>,
    /// `node_wwn`: world wide name of the node.
    pub node_wwn: Cell<u64>,
    /// `flags`: `SDEV_*` flags that all devices have.
    pub flags: Cell<u16>,
    /// `quirks`: per-device oddities (`SDEV_AUTOSAVE` ... `ADEV_NODOORLOCK`).
    pub quirks: Cell<u16>,
    /// `interpret_sense`: turns a transfer's sense data into an errno (`Err(ERESTART)`:
    /// retry).
    pub interpret_sense: Cell<ScsiInterpretSenseFn>,
    /// `device_softc`: the device driver's softc (its `struct device`), needed for calls to
    /// `foo_start`.
    pub device_softc: Cell<Option<NonNull<Device>>>,
    /// `bus`: the scsibus the link is on.
    pub bus: Cell<Option<&'static ScsibusSoftc>>,
    /// `inqdata`: copy of the INQUIRY data from the probe.
    pub inqdata: Cell<ScsiInquiryData>,
    /// `id`: the device identifier (`devid_alloc`), or NULL.
    pub id: Cell<Option<NonNull<Devid>>>,
    /// `queue`: transfer handlers waiting for an opening of this link. Protected by: the
    /// pool's `mtx`.
    pub queue: ScsiRunq,
    /// `running`: the link run queue semaphore. Protected by: the pool's `mtx`.
    pub running: Cell<u32>,
    /// `pending`: openings of this link in use. Protected by: the pool's `mtx`.
    pub pending: Cell<u16>,
    /// `pool`: where the link's openings come from.
    pub pool: Cell<Option<&'static ScsiIopool>>,
}

impl ScsiLink {
    /// A link with every member zero (`malloc(..., M_ZERO)`), except `interpret_sense`,
    /// which is `scsi_interpret_sense` (see the module's deviations).
    pub const fn new() -> Self {
        Self {
            bus_list: SlistEntry::new(),
            state: Cell::new(0),
            target: Cell::new(0),
            lun: Cell::new(0),
            openings: Cell::new(0),
            port_wwn: Cell::new(0),
            node_wwn: Cell::new(0),
            flags: Cell::new(0),
            quirks: Cell::new(0),
            interpret_sense: Cell::new(scsi_interpret_sense),
            device_softc: Cell::new(None),
            bus: Cell::new(None),
            inqdata: Cell::new(ScsiInquiryData::new()),
            id: Cell::new(None),
            queue: ScsiRunq::new(),
            running: Cell::new(0),
            pending: Cell::new(0),
            pool: Cell::new(None),
        }
    }

    /// `link->pool`, which `scsi_probe_link` always sets before the link is used.
    pub fn pool(&self) -> &'static ScsiIopool {
        match self.pool.get() {
            Some(pool) => pool,
            None => panic(format_args!("scsi_link {:p} has no iopool", self)),
        }
    }

    /// `link->bus`, which `scsi_probe_link` always sets before the link is used.
    pub fn bus(&self) -> &'static ScsibusSoftc {
        match self.bus.get() {
            Some(bus) => bus,
            None => panic(format_args!("scsi_link {:p} has no bus", self)),
        }
    }

    /// `link->id` (`NULL` is `None`).
    pub fn id(&self) -> Option<&Devid> {
        // SAFETY: a non-NULL `id` is a `devid_alloc` allocation the link holds a reference
        // to until `scsi_detach_lun` frees it.
        self.id.get().map(|d| unsafe { d.as_ref() })
    }
}

impl Default for ScsiLink {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `SLIST_HEAD(, scsi_link) sc_link_list`: the links of a bus, through `bus_list`.
    pub ScsiLinkBusList: ScsiLink, bus_list => SlistEntry<ScsiLink>
);

/// `struct scsi_inquiry_pattern`: matching information for `scsi_inqmatch()`; the more
/// things match, the higher the configuration priority.
#[derive(Clone, Copy, Debug)]
pub struct ScsiInquiryPattern {
    /// `type`: the device type (`T_*`).
    pub r#type: u8,
    /// `removable`: `T_REMOV` or `T_FIXED`.
    pub removable: i32,
    /// `vendor`.
    pub vendor: &'static [u8],
    /// `product`.
    pub product: &'static [u8],
    /// `revision`.
    pub revision: &'static [u8],
}

/// `struct scsibus_attach_args`: what an adapter tells `scsibus` when it attaches one
/// (`config_found(self, &saa, scsiprint)`).
#[derive(Clone, Copy)]
pub struct ScsibusAttachArgs {
    /// `saa_adapter`: the adapter's entry points.
    pub saa_adapter: Option<&'static ScsiAdapter>,
    /// `saa_adapter_softc`: the adapter's state, for its entry points
    /// (`link->bus->sb_adapter_softc`).
    pub saa_adapter_softc: *mut c_void,
    /// `saa_pool`: the adapter's openings, or `None` for a pool per link.
    pub saa_pool: Option<&'static ScsiIopool>,
    /// `saa_wwpn`: world wide port name.
    pub saa_wwpn: u64,
    /// `saa_wwnn`: world wide node name.
    pub saa_wwnn: u64,
    /// `saa_quirks`: quirks of every link.
    pub saa_quirks: u16,
    /// `saa_flags`: flags of every link.
    pub saa_flags: u16,
    /// `saa_openings`: openings per link.
    pub saa_openings: u16,
    /// `saa_adapter_target`: the adapter's own target, or `SDEV_NO_ADAPTER_TARGET`.
    pub saa_adapter_target: u16,
    /// `saa_adapter_buswidth`: targets on the bus.
    pub saa_adapter_buswidth: u16,
    /// `saa_luns`: LUNs per target.
    pub saa_luns: u8,
}

impl ScsibusAttachArgs {
    /// All zero (`memset(&saa, 0, sizeof(saa))`).
    pub const fn new() -> Self {
        Self {
            saa_adapter: None,
            saa_adapter_softc: ptr::null_mut(),
            saa_pool: None,
            saa_wwpn: 0,
            saa_wwnn: 0,
            saa_quirks: 0,
            saa_flags: 0,
            saa_openings: 0,
            saa_adapter_target: 0,
            saa_adapter_buswidth: 0,
            saa_luns: 0,
        }
    }
}

impl Default for ScsibusAttachArgs {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct scsibus_softc`: one per SCSI bus. It reaches the links of the bus, and keeps the
/// adapter's template values that initialise every new link.
#[repr(C)]
pub struct ScsibusSoftc {
    /// `sc_dev`.
    pub sc_dev: Device,
    /// `sc_link_list`: the bus's links.
    pub sc_link_list: SlistHead<ScsiLinkBusList>,
    /// `sb_adapter_softc`: `saa_adapter_softc`.
    pub sb_adapter_softc: Cell<*mut c_void>,
    /// `sb_adapter`: `saa_adapter`.
    pub sb_adapter: Cell<Option<&'static ScsiAdapter>>,
    /// `sb_pool`: `saa_pool`.
    pub sb_pool: Cell<Option<&'static ScsiIopool>>,
    /// `sb_quirks`.
    pub sb_quirks: Cell<u16>,
    /// `sb_flags`.
    pub sb_flags: Cell<u16>,
    /// `sb_openings`.
    pub sb_openings: Cell<u16>,
    /// `sb_adapter_buswidth`.
    pub sb_adapter_buswidth: Cell<u16>,
    /// `sb_adapter_target`.
    pub sb_adapter_target: Cell<u16>,
    /// `sb_luns`.
    pub sb_luns: Cell<u8>,
}

impl ScsibusSoftc {
    /// `sb->sb_adapter`, which `scsibusattach` sets from the attach arguments.
    pub fn adapter(&self) -> &'static ScsiAdapter {
        match self.sb_adapter.get() {
            Some(adapter) => adapter,
            None => panic(format_args!("{}: no scsi_adapter", self.sc_dev.xname())),
        }
    }
}

// SAFETY: `#[repr(C)]` with the `Device` first; every other member is a `Cell` of an
// integer, a raw pointer or an `Option` of a reference, or a list head of null pointers, all
// valid as zero bytes.
unsafe impl Softc for ScsibusSoftc {}

/// `struct scsi_attach_args`: what `scsibus` tells a device driver it attaches.
#[derive(Clone, Copy)]
pub struct ScsiAttachArgs {
    /// `sa_sc_link`: the device's link.
    pub sa_sc_link: &'static ScsiLink,
}

/// `struct scsi_xfer`: one SCSI transaction, with where it comes from and the device and
/// adapter it is for (through the link).
pub struct ScsiXfer {
    /// `xfer_list`: for the adapter's own queues.
    pub xfer_list: SimpleqEntry<ScsiXfer>,
    /// `flags`: `SCSI_*`.
    pub flags: Cell<i32>,
    /// `sc_link`: all about our device and adapter.
    pub sc_link: Cell<Option<&'static ScsiLink>>,
    /// `retries`: the number of times to retry.
    pub retries: Cell<i32>,
    /// `timeout`: in milliseconds.
    pub timeout: Cell<i32>,
    /// `cmd`: the SCSI command to execute.
    pub cmd: Cell<ScsiGeneric>,
    /// `cmdlen`: how long it is.
    pub cmdlen: Cell<i32>,
    /// `data`: DMA address or a uio address (see [`set_data`](Self::set_data)).
    data: Cell<*mut u8>,
    /// `datalen`: data length (blank if uio).
    datalen: Cell<i32>,
    /// `resid`: how much of the buffer was not touched.
    pub resid: Cell<usize>,
    /// `error`: an `XS_*` value.
    pub error: Cell<i32>,
    /// `bp`: the buffer, if the transfer is associated with one.
    pub bp: Cell<Option<&'static Buf>>,
    /// `sense`: 18 bytes.
    pub sense: Cell<ScsiSenseData>,
    /// `status`: the SCSI status byte.
    pub status: Cell<u8>,
    /// `stimeout`: a timeout for the adapter to use for the command.
    pub stimeout: Timeout,
    /// `cookie`: the issuer's (`scsi_xs_sync`'s mutex, `sd`'s buf).
    pub cookie: Cell<*mut c_void>,
    /// `done`: what `scsi_done` calls.
    pub done: Cell<Option<fn(xs: &'static ScsiXfer)>>,
    /// `io`: the adapter's I/O resource.
    pub io: Cell<Option<ScsiIo>>,
}

impl ScsiXfer {
    /// A zeroed transfer, as `pool_get(&scsi_xfer_pool, PR_ZERO)` returns it.
    pub const fn new() -> Self {
        Self {
            xfer_list: SimpleqEntry::new(),
            flags: Cell::new(0),
            sc_link: Cell::new(None),
            retries: Cell::new(0),
            timeout: Cell::new(0),
            cmd: Cell::new(ScsiGeneric {
                opcode: 0,
                bytes: [0; 15],
            }),
            cmdlen: Cell::new(0),
            data: Cell::new(ptr::null_mut()),
            datalen: Cell::new(0),
            resid: Cell::new(0),
            error: Cell::new(0),
            bp: Cell::new(None),
            sense: Cell::new(ScsiSenseData::new()),
            status: Cell::new(0),
            stimeout: Timeout::zeroed(),
            cookie: Cell::new(ptr::null_mut()),
            done: Cell::new(None),
            io: Cell::new(None),
        }
    }

    /// `xs->sc_link`, which `scsi_xs_get` always sets.
    pub fn link(&self) -> &'static ScsiLink {
        match self.sc_link.get() {
            Some(link) => link,
            None => panic(format_args!("scsi_xfer {:p} has no link", self)),
        }
    }

    /// `xs->data`.
    pub fn data(&self) -> *mut u8 {
        self.data.get()
    }

    /// `xs->datalen`.
    pub fn datalen(&self) -> i32 {
        self.datalen.get()
    }

    /// `xs->data = data; xs->datalen = datalen;`.
    ///
    /// # Safety
    ///
    /// Unless `datalen` is 0, `data` is valid for reads and writes of `datalen` bytes until
    /// the transfer completes (its `done` has run, or `scsi_xs_sync` has returned) or the
    /// data is set again, and nothing but this transfer (the adapter, `scsi_base`) reads or
    /// writes those bytes in that time.
    pub unsafe fn set_data(&self, data: *mut u8, datalen: i32) {
        self.data.set(data);
        self.datalen.set(datalen);
    }

    /// `xs->data = NULL; xs->datalen = 0;`.
    pub fn clear_data(&self) {
        self.data.set(ptr::null_mut());
        self.datalen.set(0);
    }

    /// The data as a byte slice (empty when there is none).
    ///
    /// # Safety
    ///
    /// The caller is the transfer's current owner (the adapter between `scsi_cmd` and
    /// `scsi_done`, or the issuer afterwards) and holds no other slice of the data while this
    /// one lives.
    #[allow(clippy::mut_from_ref)] // the data is not the transfer's memory; see `set_data`
    pub unsafe fn data_slice(&self) -> &mut [u8] {
        let (data, len) = (self.data.get(), self.datalen.get());
        if data.is_null() || len <= 0 {
            return &mut [];
        }
        // SAFETY: `set_data`'s contract makes `len` bytes at `data` valid and reserved for
        // this transfer; the caller's makes this the only slice of them.
        unsafe { slice::from_raw_parts_mut(data, len as usize) }
    }

    /// `memcpy(&xs->cmd, &cdb, sizeof(cdb))`: writes a CDB into the front of `cmd`.
    pub fn set_cmd<T: ScsiWire>(&self, cdb: &T) {
        let mut cmd = self.cmd.get();
        cmd.as_bytes_mut()[..size_of::<T>()].copy_from_slice(cdb.as_bytes());
        self.cmd.set(cmd);
    }

    /// `*(struct T *)&xs->cmd`: a copy of `cmd` read as the CDB `T`.
    pub fn cmd_as<T: ScsiWire>(&self) -> T {
        *wire_ref::<T>(self.cmd.get().as_bytes())
    }

    /// `cmd = (struct T *)&xs->cmd; cmd->... = ...;`: changes `cmd` through a `T` view.
    pub fn with_cmd<T: ScsiWire, R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let mut cmd = self.cmd.get();
        let r = f(wire_mut::<T>(cmd.as_bytes_mut()));
        self.cmd.set(cmd);
        r
    }
}

impl Default for ScsiXfer {
    fn default() -> Self {
        Self::new()
    }
}

queue_adapter!(
    /// `SIMPLEQ_HEAD(scsi_xfer_list, scsi_xfer)`: an adapter's queue of transfers, through
    /// `xfer_list`.
    pub ScsiXferListEntries: ScsiXfer, xfer_list => SimpleqEntry<ScsiXfer>
);

/// `struct scsi_xfer_list`.
pub type ScsiXferList = SimpleqHead<ScsiXferListEntries>;

/// `_lto2b`: stores the low 16 bits of `val` big-endian in `bytes[0..2]`.
pub const fn _lto2b(val: u32, bytes: &mut [u8]) {
    bytes[0] = (val >> 8) as u8;
    bytes[1] = val as u8;
}

/// `_lto3b`: stores the low 24 bits of `val` big-endian in `bytes[0..3]`.
pub const fn _lto3b(val: u32, bytes: &mut [u8]) {
    bytes[0] = (val >> 16) as u8;
    bytes[1] = (val >> 8) as u8;
    bytes[2] = val as u8;
}

/// `_lto4b`: stores `val` big-endian in `bytes[0..4]`.
pub const fn _lto4b(val: u32, bytes: &mut [u8]) {
    bytes[0] = (val >> 24) as u8;
    bytes[1] = (val >> 16) as u8;
    bytes[2] = (val >> 8) as u8;
    bytes[3] = val as u8;
}

/// `_lto8b`: stores `val` big-endian in `bytes[0..8]`.
pub const fn _lto8b(val: u64, bytes: &mut [u8]) {
    let mut i = 0;
    while i < 8 {
        bytes[i] = (val >> (56 - 8 * i)) as u8;
        i += 1;
    }
}

/// `_2btol`: the big-endian 16-bit number in `bytes[0..2]`.
pub const fn _2btol(bytes: &[u8]) -> u32 {
    ((bytes[0] as u32) << 8) | bytes[1] as u32
}

/// `_3btol`: the big-endian 24-bit number in `bytes[0..3]`.
pub const fn _3btol(bytes: &[u8]) -> u32 {
    ((bytes[0] as u32) << 16) | ((bytes[1] as u32) << 8) | bytes[2] as u32
}

/// `_4btol`: the big-endian 32-bit number in `bytes[0..4]`.
pub const fn _4btol(bytes: &[u8]) -> u32 {
    ((bytes[0] as u32) << 24)
        | ((bytes[1] as u32) << 16)
        | ((bytes[2] as u32) << 8)
        | bytes[3] as u32
}

/// `_5btol`: the big-endian 40-bit number in `bytes[0..5]`.
pub const fn _5btol(bytes: &[u8]) -> u64 {
    ((bytes[0] as u64) << 32)
        | ((bytes[1] as u64) << 24)
        | ((bytes[2] as u64) << 16)
        | ((bytes[3] as u64) << 8)
        | bytes[4] as u64
}

/// `_8btol`: the big-endian 64-bit number in `bytes[0..8]`.
pub const fn _8btol(bytes: &[u8]) -> u64 {
    let mut rv = 0u64;
    let mut i = 0;
    while i < 8 {
        rv = (rv << 8) | bytes[i] as u64;
        i += 1;
    }
    rv
}

/// `DEVID_CMP(_a, _b)`: whether two device identifiers name the same device (both non-NULL,
/// and the same object or the same non-`DEVID_NONE` type, length and bytes).
///
/// # Safety
///
/// Each of `a` and `b` that is `Some` is the header of a `devid_alloc` allocation (see
/// [`Devid::id`]).
pub unsafe fn devid_cmp(a: Option<&Devid>, b: Option<&Devid>) -> bool {
    let (Some(a), Some(b)) = (a, b) else {
        return false;
    };
    // SAFETY: the caller's contract, for both.
    ptr::eq(a, b)
        || (a.d_type != DEVID_NONE
            && a.d_type == b.d_type
            && a.d_len == b.d_len
            && unsafe { a.id() == b.id() })
}

/// `SID_ANSII_REV(x)`: the SCSI version an INQUIRY reply claims (`SCSI_REV_*`).
pub const fn sid_ansii_rev(x: &ScsiInquiryData) -> u8 {
    x.version & SID_ANSII
}

/// `SID_RESPONSE_FORMAT(x)`: the INQUIRY response data format.
pub const fn sid_response_format(x: &ScsiInquiryData) -> u8 {
    x.response_format & SID_RESPONSE_DATA_FMT
}

/// `scsi_probe(sb, target, lun)` (`scsiconf.c`): probes the bus, a target or a LUN (-1:
/// all). Not ported yet: reports the gap.
pub fn scsi_probe(sb: &'static ScsibusSoftc, target: i32, lun: i32) -> Result<(), Errno> {
    let _ = (sb, target, lun);
    Err(unported!("scsi_probe (scsiconf.c)"))
}

/// `scsi_detach(sb, target, lun, flags)` (`scsiconf.c`): detaches the bus's devices, a
/// target's or a LUN's (-1: all). Not ported yet: reports the gap.
pub fn scsi_detach(
    sb: &'static ScsibusSoftc,
    target: i32,
    lun: i32,
    flags: i32,
) -> Result<(), Errno> {
    let _ = (sb, target, lun, flags);
    Err(unported!("scsi_detach (scsiconf.c)"))
}

#[cfg(test)]
mod tests;
