/*	$OpenBSD: scsiconf.h,v 1.202 2023/05/10 15:28:26 krw Exp $	*/
/*	$NetBSD: scsiconf.h,v 1.35 1997/04/02 02:29:38 mycroft Exp $	*/
/*	$OpenBSD: scsiconf.c,v 1.255 2025/09/16 12:18:10 hshoexer Exp $	*/
/*	$NetBSD: scsiconf.c,v 1.57 1996/05/02 01:09:01 neil Exp $	*/
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

/*
 * Copyright (c) 1994 Charles Hannum.  All rights reserved.
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
//! and the big-endian helpers (`_lto4b`, `_4btol`, ...) every CDB uses; and `scsiconf.c`,
//! the `scsibus` driver: it attaches above an adapter, probes every target and LUN with
//! INQUIRY (REPORT LUNS when the device knows it), applies the quirk table, finds a device
//! identifier in the VPD pages, attaches the device driver that matches, and detaches,
//! suspends and resumes the links.
//!
//! Upstream: sys/scsi/scsiconf.h @ 3ce1f3f79392
//! Upstream: sys/scsi/scsiconf.c @ 3ce1f3f79392
//!
//! The functions the header declares live in `scsi_base.rs` (the transfer and pool
//! machinery, `scsi_xs_get`, `scsi_xs_exec`, `scsi_done`, ...), in `scsi_ioctl.rs`
//! (`scsi_do_ioctl`) and, for `scsiconf.c` (probe, attach, detach, `scsiprint`,
//! `scsi_inqmatch`), at the end of this file.
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
//! - The prototypes of the header are not repeated: Rust needs none.
//!
//! `scsiconf.c`:
//! - bio(4) is configured (`NBIO` is 1, `dev/bio.rs`): `scsibusattach` registers
//!   [`scsibusbioctl`] with `bio_register` and `scsibusdetach` calls `bio_unregister`.
//! - `NMPATH` (`mpath.h`) is only included by the C file, which uses nothing of it; mpath(4)
//!   is not ported (`subr_autoconf.rs`).
//! - `SCSIDEBUG` is not configured and `scsi_debug.h` is not ported: the `SC_DEBUG` sites
//!   and the `#ifdef SCSIDEBUG` blocks (debug masks in `scsi_probe_link`, the protocol
//!   level and the link's state in `scsi_print_link`, the match trace in `scsi_inqmatch`)
//!   are comments.
//! - `dma_alloc(9)` (`kern/dma_alloc.c`) is not ported: the INQUIRY, REPORT LUNS and VPD
//!   buffers come from `malloc(9)` through `DmaBuf`, freed when dropped (`dma_free`); the
//!   `M_TEMP` identifier buffer of `scsi_devid_pg80` is one too.
//! - The integer results are `Result<(), Errno>`; the "nothing there" `EINVAL` the probe
//!   functions pass up is `Err(EINVAL)`, and the `goto bad` of `scsi_probe_link` with an
//!   error of 0 is `Ok(())` after the link is freed.
//! - The C global `scsi_autoconf` is a `static` [`AtomicI32`] of the same name (beside the
//!   `SCSI_AUTOCONF` flag value it starts with). `atomic_setbits_int(&link->state, ...)` is
//!   a `Cell` update (under the kernel lock, as the rest of the link).
//! - [`scsi_inqmatch`] takes a slice of anything that is or begins with an
//!   [`ScsiInquiryPattern`] (`AsRef`) where the C takes a base pointer, a count and an
//!   element size, and returns the best entry and its priority. A pattern longer than its
//!   field goes on comparing the bytes that follow it, as the C's `bcmp` does
//!   (`"MATSHITA CR-574"` covers vendor and product).
//! - [`scsi_strvis`] stops writing where `dst` ends (the C trusts it to hold `4 * len + 1`
//!   bytes).
//! - `scsi_activate_link` does nothing for a link without a device (the C would call
//!   `config_deactivate`/`config_suspend` on NULL).
//! - `scsi_get_target_luns` never reads past the 256 entries of the REPORT LUNS buffer
//!   when the device claims a longer list (the C would).
//! - `scsi_detach_link` is `unsafe`: it frees the link.
//! - `devid_alloc` copies at most `id.len()` bytes; any missing ones stay zero.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::{self, NonNull};
use core::slice;

use core::mem::{offset_of, size_of};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::dev::bio::{bio_register, bio_unregister};
use crate::kern::kern_malloc::{free, malloc};
use crate::kern::subr_autoconf::{
    config_attach, config_deactivate, config_detach, config_search, config_suspend,
};
use crate::kern::subr_prf::{Str, panic};
use crate::queue_adapter;
use crate::scsi::scsi_all::{
    REPORT_NORMAL, RPL_LUNDATA_SIZE, RPL_LUNDATA_T0LUN, SI_PG_DEVID, SI_PG_SERIAL, SI_PG_SUPPORTED,
    SID_ANSII, SID_CmdQue, SID_QUAL, SID_QUAL_BAD_LU, SID_QUAL_LU_OFFLINE, SID_QUAL_RSVD,
    SID_REMOVABLE, SID_RESPONSE_DATA_FMT, SID_SCSI2_HDRLEN, SID_Sync, SID_TYPE, SID_WBus16,
    ScsiGeneric, ScsiInquiryData, ScsiLunArray, ScsiReportLunsData, ScsiSenseData, ScsiVpdDevidHdr,
    ScsiVpdHdr, ScsiWire, T_CDROM, T_DIRECT, T_FIXED, T_NODEVICE, T_REMOV, T_SEQUENTIAL,
    VPD_DEVID_ASSOC_LU, VPD_DEVID_CODE_ASCII, VPD_DEVID_CODE_UTF8, VPD_DEVID_TYPE_EUI64,
    VPD_DEVID_TYPE_NAA, VPD_DEVID_TYPE_T10, vpd_devid_assoc, vpd_devid_code, vpd_devid_type,
    wire_mut, wire_ref,
};
use crate::scsi::scsi_base::{
    scsi_default_get, scsi_default_put, scsi_init, scsi_inquire, scsi_inquire_vpd,
    scsi_interpret_sense, scsi_iopool_destroy, scsi_iopool_init, scsi_link_shutdown,
    scsi_report_luns, scsi_test_unit_ready,
};
use crate::sys::buf::Buf;
use crate::sys::device::{
    CD_COCOVM, CfMatch, Cfattach, Cfdriver, DETACH_FORCE, DV_DULL, DVACT_DEACTIVATE, Device, Softc,
    UNCONF,
};
use crate::sys::errno::Errno::{self, *};
use crate::sys::ioctl::ioctl_arg;
use crate::sys::malloc::{M_CANFAIL, M_DEVBUF, M_NOWAIT, M_WAITOK, M_ZERO};
use crate::sys::mutex::Mutex;
use crate::sys::queue::{SimpleqEntry, SimpleqHead, SlistEntry, SlistHead, TailqEntry, TailqHead};
use crate::sys::scsiio::{SBIOCDETACH, SBIOCPROBE, SbiocDevice};
use crate::sys::systm::COLD;
use crate::sys::timeout::Timeout;
use crate::{kassert, kprintf};

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
    pub dev_minphys: Option<fn(bp: &Buf, link: &'static ScsiLink)>,
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

/*
 * scsiconf.c
 */

/// `struct scsi_quirk_inquiry_pattern`: an inquiry pattern and the quirks of the devices
/// it matches.
#[derive(Clone, Copy, Debug)]
pub struct ScsiQuirkInquiryPattern {
    /// `pattern`.
    pub pattern: ScsiInquiryPattern,
    /// `quirks`: `SDEV_*`/`ADEV_*` quirk bits.
    pub quirks: u16,
}

impl AsRef<ScsiInquiryPattern> for ScsiInquiryPattern {
    fn as_ref(&self) -> &ScsiInquiryPattern {
        self
    }
}

impl AsRef<ScsiInquiryPattern> for ScsiQuirkInquiryPattern {
    fn as_ref(&self) -> &ScsiInquiryPattern {
        &self.pattern
    }
}

/// A `dma_alloc(9)` buffer, given back on drop (`dma_free`).
///
/// `kern/dma_alloc.c` is not ported: the bytes come from `malloc(9)` (`M_DEVBUF`), which
/// both archs' `bus_dma` can map, always zeroed (`PR_ZERO`).
pub(crate) struct DmaBuf {
    addr: NonNull<u8>,
    len: usize,
}

impl DmaBuf {
    /// `dma_alloc(len, flags | PR_ZERO)`, `flags` being `M_WAITOK` or `M_NOWAIT`; `None`
    /// when the memory is not there.
    pub(crate) fn new(len: usize, flags: i32) -> Option<Self> {
        let addr = malloc(len.max(1), M_DEVBUF, flags | M_ZERO)?;
        Some(Self { addr, len })
    }

    /// The buffer's bytes.
    pub(crate) fn bytes(&mut self) -> &mut [u8] {
        // SAFETY: `len` bytes at `addr` are this buffer's own allocation, zeroed at first and
        // initialised since, borrowed through `self`.
        unsafe { slice::from_raw_parts_mut(self.addr.as_ptr(), self.len) }
    }

    /// The buffer read as the wire structure `T` (`(struct T *)buf`).
    pub(crate) fn wire<T: ScsiWire>(&mut self) -> &mut T {
        wire_mut::<T>(self.bytes())
    }
}

impl Drop for DmaBuf {
    fn drop(&mut self) {
        free(self.addr, M_DEVBUF, self.len.max(1));
    }
}

/// `scsi_autoconf`: the flags of the probe's commands, `SCSI_AUTOCONF` while the machine
/// is cold, 0 once a bus attaches later (a hot-plugged adapter may sleep).
#[allow(non_upper_case_globals)] // the C name; `SCSI_AUTOCONF` is the value it starts with
pub static scsi_autoconf: AtomicI32 = AtomicI32::new(SCSI_AUTOCONF);

/// `scsibus_ca`.
pub static SCSIBUS_CA: Cfattach = Cfattach {
    ca_devsize: size_of::<ScsibusSoftc>(),
    ca_match: Some(scsibusmatch),
    ca_attach: scsibusattach,
    ca_detach: Some(scsibusdetach),
    ca_activate: Some(scsibusactivate),
};

/// `scsibus_cd`.
pub static SCSIBUS_CD: Cfdriver = Cfdriver::new(b"scsibus", DV_DULL, CD_COCOVM);

/// One `scsi_quirk_patterns[]` entry.
const fn quirk(
    r#type: u8,
    removable: i32,
    vendor: &'static [u8],
    product: &'static [u8],
    revision: &'static [u8],
    quirks: u16,
) -> ScsiQuirkInquiryPattern {
    ScsiQuirkInquiryPattern {
        pattern: ScsiInquiryPattern {
            r#type,
            removable,
            vendor,
            product,
            revision,
        },
        quirks,
    }
}

/// `scsi_quirk_patterns[]`: devices the probe knows to be odd.
pub static SCSI_QUIRK_PATTERNS: [ScsiQuirkInquiryPattern; 33] = [
    quirk(
        T_CDROM,
        T_REMOV,
        b"PLEXTOR",
        b"CD-ROM PX-40TS",
        b"1.01",
        SDEV_NOSYNC,
    ),
    quirk(
        T_DIRECT,
        T_FIXED,
        b"MICROP  ",
        b"1588-15MBSUN0669",
        b"",
        SDEV_AUTOSAVE,
    ),
    quirk(
        T_DIRECT,
        T_FIXED,
        b"DEC     ",
        b"RZ55     (C) DEC",
        b"",
        SDEV_AUTOSAVE,
    ),
    quirk(
        T_DIRECT,
        T_FIXED,
        b"EMULEX  ",
        b"MD21/S2     ESDI",
        b"A00",
        SDEV_AUTOSAVE,
    ),
    quirk(T_DIRECT, T_FIXED, b"IBMRAID ", b"0662S", b"", SDEV_AUTOSAVE),
    quirk(T_DIRECT, T_FIXED, b"IBM     ", b"0663H", b"", SDEV_AUTOSAVE),
    quirk(T_DIRECT, T_FIXED, b"IBM", b"0664", b"", SDEV_AUTOSAVE),
    quirk(
        T_DIRECT,
        T_FIXED,
        b"IBM     ",
        b"H3171-S2",
        b"",
        SDEV_AUTOSAVE,
    ),
    quirk(T_DIRECT, T_FIXED, b"IBM     ", b"KZ-C", b"", SDEV_AUTOSAVE),
    // Broken IBM disk
    quirk(T_DIRECT, T_FIXED, b"", b"DFRSS2F", b"", SDEV_AUTOSAVE),
    quirk(
        T_DIRECT,
        T_FIXED,
        b"QUANTUM ",
        b"ELS85S          ",
        b"",
        SDEV_AUTOSAVE,
    ),
    quirk(T_DIRECT, T_REMOV, b"iomega", b"jaz 1GB", b"", SDEV_NOTAGS),
    quirk(T_DIRECT, T_FIXED, b"MICROP", b"4421-07", b"", SDEV_NOTAGS),
    quirk(
        T_DIRECT,
        T_FIXED,
        b"SEAGATE",
        b"ST150176LW",
        b"0002",
        SDEV_NOTAGS,
    ),
    quirk(T_DIRECT, T_FIXED, b"HP", b"C3725S", b"", SDEV_NOTAGS),
    quirk(T_DIRECT, T_FIXED, b"IBM", b"DCAS", b"", SDEV_NOTAGS),
    quirk(
        T_SEQUENTIAL,
        T_REMOV,
        b"SONY    ",
        b"SDT-5000        ",
        b"3.",
        SDEV_NOSYNC | SDEV_NOWIDE,
    ),
    quirk(
        T_SEQUENTIAL,
        T_REMOV,
        b"WangDAT ",
        b"Model 1300      ",
        b"02.4",
        SDEV_NOSYNC | SDEV_NOWIDE,
    ),
    quirk(
        T_SEQUENTIAL,
        T_REMOV,
        b"WangDAT ",
        b"Model 2600      ",
        b"01.7",
        SDEV_NOSYNC | SDEV_NOWIDE,
    ),
    quirk(
        T_SEQUENTIAL,
        T_REMOV,
        b"WangDAT ",
        b"Model 3200      ",
        b"02.2",
        SDEV_NOSYNC | SDEV_NOWIDE,
    ),
    // ATAPI device quirks
    quirk(T_CDROM, T_REMOV, b"CR-2801TE", b"", b"1.07", ADEV_NOSENSE),
    quirk(
        T_CDROM,
        T_REMOV,
        b"CREATIVECD3630E",
        b"",
        b"AC101",
        ADEV_NOSENSE,
    ),
    quirk(T_CDROM, T_REMOV, b"FX320S", b"", b"q01", ADEV_NOSENSE),
    quirk(T_CDROM, T_REMOV, b"GCD-R580B", b"", b"1.00", ADEV_LITTLETOC),
    quirk(
        T_CDROM,
        T_REMOV,
        b"MATSHITA CR-574",
        b"",
        b"1.02",
        ADEV_NOCAPACITY,
    ),
    quirk(
        T_CDROM,
        T_REMOV,
        b"MATSHITA CR-574",
        b"",
        b"1.06",
        ADEV_NOCAPACITY,
    ),
    quirk(
        T_CDROM,
        T_REMOV,
        b"Memorex CRW-2642",
        b"",
        b"1.0g",
        ADEV_NOSENSE,
    ),
    quirk(
        T_CDROM,
        T_REMOV,
        b"SANYO CRD-256P",
        b"",
        b"1.02",
        ADEV_NOCAPACITY,
    ),
    quirk(
        T_CDROM,
        T_REMOV,
        b"SANYO CRD-254P",
        b"",
        b"1.02",
        ADEV_NOCAPACITY,
    ),
    quirk(
        T_CDROM,
        T_REMOV,
        b"SANYO CRD-S54P",
        b"",
        b"1.08",
        ADEV_NOCAPACITY,
    ),
    quirk(
        T_CDROM,
        T_REMOV,
        b"CD-ROM  CDR-S1",
        b"",
        b"1.70",
        ADEV_NOCAPACITY,
    ), // Sanyo
    quirk(
        T_CDROM,
        T_REMOV,
        b"CD-ROM  CDR-N16",
        b"",
        b"1.25",
        ADEV_NOCAPACITY,
    ), // Sanyo
    quirk(
        T_CDROM,
        T_REMOV,
        b"UJDCD8730",
        b"",
        b"1.14",
        ADEV_NODOORLOCK,
    ), // Acer
];

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

/*
 * scsiconf.c
 */

/// The `scsibus` softc of `dev`, for as long as the bus is attached.
fn scsibus_softc(dev: &Device) -> &'static ScsibusSoftc {
    // SAFETY: `dev` is a scsibus device (these are `SCSIBUS_CA`'s functions, whose
    // `ca_devsize` is a `ScsibusSoftc`). The softc lives until `config_detach` frees it,
    // after `scsibusdetach` has detached and freed every link that holds it, so no
    // `&'static` handed out here outlives it.
    unsafe { &*ptr::from_ref(dev.softc::<ScsibusSoftc>()) }
}

/// `scsiprint`: the print function of the adapters' `config_found` (only `scsibus`es can
/// attach to `scsi`s).
pub fn scsiprint(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    let _ = aux;
    if let Some(pnp) = pnp {
        kprintf!("scsibus at {}", Str(pnp));
    }
    UNCONF
}

/// `scsibusmatch`: a scsibus attaches to every `scsi` attribute.
pub fn scsibusmatch(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let _ = (parent, match_, aux);
    1
}

/// `scsibusattach`: the routine called by the adapter boards to get all their devices
/// configured in.
pub fn scsibusattach(parent: Option<&Device>, self_: &Device, aux: *mut c_void) {
    let _ = parent;
    let sb = scsibus_softc(self_);
    // SAFETY: the `scsi` attribute's attach arguments are always a `ScsibusAttachArgs`
    // (the adapter's `config_found(self, &saa, scsiprint)`), alive during the attach.
    let saa = unsafe { &*aux.cast::<ScsibusAttachArgs>() };

    if !COLD.load(Ordering::Relaxed) {
        scsi_autoconf.store(0, Ordering::Relaxed);
    }

    sb.sc_link_list.init();
    sb.sb_adapter_softc.set(saa.saa_adapter_softc);
    sb.sb_adapter.set(saa.saa_adapter);
    sb.sb_pool.set(saa.saa_pool);
    sb.sb_quirks.set(saa.saa_quirks);
    sb.sb_flags.set(saa.saa_flags);
    sb.sb_openings.set(saa.saa_openings);
    sb.sb_adapter_buswidth.set(saa.saa_adapter_buswidth);
    sb.sb_adapter_target.set(saa.saa_adapter_target);
    sb.sb_luns.set(saa.saa_luns);

    if sb.sb_adapter_buswidth.get() == 0 {
        sb.sb_adapter_buswidth.set(8);
    }
    if sb.sb_luns.get() == 0 {
        sb.sb_luns.set(8);
    }

    kprintf!(": {} targets", sb.sb_adapter_buswidth.get());
    if sb.sb_adapter_target.get() < sb.sb_adapter_buswidth.get() {
        kprintf!(", initiator {}", sb.sb_adapter_target.get());
    }
    if saa.saa_wwpn != 0x0 && saa.saa_wwnn != 0x0 {
        kprintf!(", WWPN {:016x}, WWNN {:016x}", saa.saa_wwpn, saa.saa_wwnn);
    }
    kprintf!("\n");

    // Initialize shared data.
    scsi_init();

    sb.sc_link_list.init();

    // NBIO > 0.
    if bio_register(&sb.sc_dev, scsibusbioctl).is_err() {
        kprintf!("{}: unable to register bio\n", sb.sc_dev.xname());
    }

    let _ = scsi_probe_bus(sb);
}

/// `scsibusactivate`.
pub fn scsibusactivate(dev: &Device, act: i32) -> Result<(), Errno> {
    scsi_activate_bus(scsibus_softc(dev), act)
}

/// `scsibusdetach`: detaches every link of the bus.
pub fn scsibusdetach(dev: &Device, r#type: i32) -> Result<(), Errno> {
    let sb = scsibus_softc(dev);

    // NBIO > 0.
    bio_unregister(&sb.sc_dev);

    scsi_detach_bus(sb, r#type)?;

    kassert!(sb.sc_link_list.is_empty());

    Ok(())
}

/// `scsibussubmatch`: checks the `target` and `lun` locators before the driver's match.
pub fn scsibussubmatch(parent: Option<&Device>, match_: &CfMatch, aux: *mut c_void) -> i32 {
    let cf = match_.cfdata();
    // SAFETY: `scsi_probe_link` searches with a `ScsiAttachArgs` as the aux.
    let sa = unsafe { &*aux.cast::<ScsiAttachArgs>() };
    let link = sa.sa_sc_link;
    let loc = |i: usize| cf.cf_loc.get(i).copied().unwrap_or(-1);

    if loc(0) != -1 && loc(0) != i64::from(link.target.get()) {
        return 0;
    }
    if loc(1) != -1 && loc(1) != i64::from(link.lun.get()) {
        return 0;
    }

    cf.cf_attach
        .ca_match
        .map_or(0, |ca_match| ca_match(parent, match_, aux))
}

/// `scsibussubprint`: prints out autoconfiguration information for a subdevice.
///
/// This is a slight abuse of 'standard' autoconfiguration semantics, because 'print'
/// functions don't normally print the colon and device information. However, in this case
/// that's better than either printing redundant information before the attach message, or
/// having the device driver call a special function to print out the standard device
/// information.
pub fn scsibussubprint(aux: *mut c_void, pnp: Option<&[u8]>) -> i32 {
    // SAFETY: as in `scsibussubmatch`.
    let sa = unsafe { &*aux.cast::<ScsiAttachArgs>() };

    if let Some(pnp) = pnp {
        kprintf!("{}", Str(pnp));
    }

    scsi_print_link(sa.sa_sc_link);

    UNCONF
}

/// `scsibusbioctl`: the bio(4) ioctls of a bus (`SBIOCPROBE`, `SBIOCDETACH`).
///
/// `scsibusattach` registers it with `bio_register` when bio(4) is configured (`NBIO > 0`).
pub fn scsibusbioctl(dev: &Device, cmd: u64, addr: &mut [u8]) -> Result<(), Errno> {
    let sb = scsibus_softc(dev);

    match cmd {
        SBIOCPROBE => {
            let sdev = ioctl_arg::<SbiocDevice>(addr);
            scsi_probe(sb, sdev.sd_target, sdev.sd_lun)
        }
        SBIOCDETACH => {
            let sdev = ioctl_arg::<SbiocDevice>(addr);
            scsi_detach(sb, sdev.sd_target, sdev.sd_lun, 0)
        }
        _ => Err(ENOTTY),
    }
}

/// `scsi_activate`: hands `act` to the devices of the bus, of a target or of one LUN (-1:
/// all).
pub fn scsi_activate(
    sb: &'static ScsibusSoftc,
    target: i32,
    lun: i32,
    act: i32,
) -> Result<(), Errno> {
    if target == -1 && lun == -1 {
        scsi_activate_bus(sb, act)
    } else if lun == -1 {
        scsi_activate_target(sb, target, act)
    } else {
        scsi_activate_lun(sb, target, lun, act)
    }
}

/// `scsi_activate_bus`: activates all links on the bus; the last error wins.
fn scsi_activate_bus(sb: &ScsibusSoftc, act: i32) -> Result<(), Errno> {
    let mut rv = Ok(());
    for link in sb.sc_link_list.iter() {
        let r = scsi_activate_link(link, act);
        if r.is_err() {
            rv = r;
        }
    }
    rv
}

/// `scsi_activate_target`: activates all links on the target.
fn scsi_activate_target(sb: &ScsibusSoftc, target: i32, act: i32) -> Result<(), Errno> {
    let mut rv = Ok(());
    for link in sb.sc_link_list.iter() {
        if i32::from(link.target.get()) == target {
            let r = scsi_activate_link(link, act);
            if r.is_err() {
                rv = r;
            }
        }
    }
    rv
}

/// `scsi_activate_lun`: activates the (target, lun) link.
fn scsi_activate_lun(
    sb: &'static ScsibusSoftc,
    target: i32,
    lun: i32,
    act: i32,
) -> Result<(), Errno> {
    match scsi_get_link(sb, target, lun) {
        Some(link) => scsi_activate_link(link, act),
        None => Ok(()),
    }
}

/// `scsi_activate_link`: marks the link dying and deactivates its device, or suspends or
/// resumes the device.
fn scsi_activate_link(link: &ScsiLink, act: i32) -> Result<(), Errno> {
    let Some(dev) = link.device_softc.get() else {
        return Ok(());
    };
    // SAFETY: a device driver stores its own attached device in the link; it detaches
    // (`scsi_detach_link`) before the link is freed.
    let dev = unsafe { dev.as_ref() };

    match act {
        DVACT_DEACTIVATE => {
            link.state.set(link.state.get() | SDEV_S_DYING);
            // The C ignores the result.
            let _ = config_deactivate(dev);
            Ok(())
        }
        _ => config_suspend(dev, act),
    }
}

/// `scsi_probe`: probes the bus, a target or one LUN (-1: all).
pub fn scsi_probe(sb: &'static ScsibusSoftc, target: i32, lun: i32) -> Result<(), Errno> {
    if target == -1 && lun == -1 {
        scsi_probe_bus(sb)
    } else if lun == -1 {
        scsi_probe_target(sb, target)
    } else {
        scsi_probe_lun(sb, target, lun)
    }
}

/// `scsi_probe_bus`: probes all possible targets on the bus.
pub fn scsi_probe_bus(sb: &'static ScsibusSoftc) -> Result<(), Errno> {
    let mut rv = Ok(());
    for target in 0..i32::from(sb.sb_adapter_buswidth.get()) {
        let r = scsi_probe_target(sb, target);
        if matches!(r, Err(e) if e != EINVAL) {
            rv = r;
        }
    }
    rv
}

/// `scsi_probe_target`: probes the LUNs the target reports, or all of them.
pub fn scsi_probe_target(sb: &'static ScsibusSoftc, target: i32) -> Result<(), Errno> {
    if target < 0 || target == i32::from(sb.sb_adapter_target.get()) {
        return Err(EINVAL);
    }

    let lunarray = scsi_get_target_luns(sb, target);
    if lunarray.count == 0 {
        return Err(EINVAL);
    }

    let mut rv = Ok(());
    for &lun in &lunarray.luns[..lunarray.count as usize] {
        let r = scsi_probe_link(sb, target, i32::from(lun), lunarray.dumbscan);
        if r == Err(EINVAL) && lunarray.dumbscan == 1 {
            return Ok(());
        }
        if matches!(r, Err(e) if e != EINVAL) {
            rv = r;
        }
    }
    rv
}

/// `scsi_probe_lun`: probes one LUN of the target (*not* a dumbscan).
pub fn scsi_probe_lun(sb: &'static ScsibusSoftc, target: i32, lun: i32) -> Result<(), Errno> {
    if target < 0 || target == i32::from(sb.sb_adapter_target.get()) || lun < 0 {
        return Err(EINVAL);
    }

    scsi_probe_link(sb, target, lun, 0)
}

/// `scsi_probe_link`: makes a link for (target, lun), asks the device what it is, and
/// attaches the driver that wants it. `Err(EINVAL)` says there is nothing at LUN 0 or the
/// device does not tell its LUNs apart (`dumbscan`).
fn scsi_probe_link(
    sb: &'static ScsibusSoftc,
    target: i32,
    lun: i32,
    dumbscan: i32,
) -> Result<(), Errno> {
    // Skip this slot if it is already attached and try the next LUN.
    if scsi_get_link(sb, target, lun).is_some() {
        return Ok(());
    }

    let Some(mem) = malloc(size_of::<ScsiLink>(), M_DEVBUF, M_NOWAIT) else {
        // SC_DEBUG(link, SDEV_DB2, ("malloc(scsi_link) failed.\n")).
        return Err(EINVAL);
    };
    let linkp = mem.cast::<ScsiLink>();
    // SAFETY: a fresh allocation of a `ScsiLink`'s size, aligned by malloc(9), written once
    // before anything else sees it. It is freed only by `scsi_detach_link` (or below, before
    // anyone else has it), so `&'static` holds until then.
    let link: &'static ScsiLink = unsafe {
        linkp.as_ptr().write(ScsiLink::new());
        linkp.as_ref()
    };

    // `ScsiLink::new` zeroes the rest: state, wwns, device_softc, inqdata, id, the queue,
    // running, pending; and sets interpret_sense to scsi_interpret_sense.
    link.target.set(target as u16);
    link.lun.set(lun as u16);
    link.openings.set(sb.sb_openings.get());
    link.flags.set(sb.sb_flags.get());
    link.quirks.set(sb.sb_quirks.get());
    link.bus.set(Some(sb));
    link.pool.set(sb.sb_pool.get());

    // SC_DEBUG(link, SDEV_DB2, ("scsi_link created.\n")).

    // Ask the adapter if this will be a valid device.
    if let Some(dev_probe) = sb.adapter().dev_probe
        && dev_probe(link).is_err()
    {
        // SC_DEBUG(link, SDEV_DB2, ("dev_probe(link) failed.\n")) for LUN 0.
        free(mem, M_DEVBUF, size_of::<ScsiLink>());
        return if lun == 0 { Err(EINVAL) } else { Ok(()) };
    }

    let flags = scsi_autoconf.load(Ordering::Relaxed);
    let rslt: Result<(), Errno> = 'bad: {
        // If we haven't been given an io pool by now then fall back to using
        // link->openings.
        if link.pool.get().is_none() {
            let Some(pmem) = malloc(size_of::<ScsiIopool>(), M_DEVBUF, M_NOWAIT) else {
                // SC_DEBUG(link, SDEV_DB2, ("malloc(pool) failed.\n")).
                break 'bad Err(ENOMEM);
            };
            let poolp = pmem.cast::<ScsiIopool>();
            // SAFETY: as for the link: a fresh, aligned allocation written once; freed only by
            // `scsi_detach_link` after `scsi_iopool_destroy`.
            let pool: &'static ScsiIopool = unsafe {
                poolp.as_ptr().write(ScsiIopool::new());
                poolp.as_ref()
            };
            // SAFETY: the default allocator ignores its cookie (the link, which outlives the
            // pool anyway).
            unsafe {
                scsi_iopool_init(
                    pool,
                    ptr::from_ref(link).cast_mut().cast(),
                    scsi_default_get,
                    scsi_default_put,
                );
            }
            link.pool.set(Some(pool));

            link.flags.set(link.flags.get() | SDEV_OWN_IOPL);
        }

        // Tell drivers that are paying attention to avoid sync/wide/tags until INQUIRY
        // data has been processed and the quirks information is complete. Some drivers set
        // bits in quirks before we get here, so just add NOTAGS, NOWIDE and NOSYNC.
        let devquirks = link.quirks.get();
        link.quirks
            .set(link.quirks.get() | SDEV_NOSYNC | SDEV_NOWIDE | SDEV_NOTAGS);

        // Ask the device what it is. (SCSIDEBUG, not configured: the scsidebug_buses,
        // _targets and _luns masks would set scsidebug_level in link->flags here.)

        if lun == 0 {
            // Clear any outstanding errors.
            let _ = scsi_test_unit_ready(
                link,
                TEST_READY_RETRIES,
                flags
                    | SCSI_IGNORE_ILLEGAL_REQUEST
                    | SCSI_IGNORE_NOT_READY
                    | SCSI_IGNORE_MEDIA_CHANGE,
            );
        }

        // Now go ask the device all about itself.
        let Some(mut inqmem) = DmaBuf::new(size_of::<ScsiInquiryData>(), M_NOWAIT) else {
            // SC_DEBUG(link, SDEV_DB2, ("dma_alloc(inqbuf) failed.\n")).
            break 'bad Err(ENOMEM);
        };

        if let Err(e) = scsi_inquire(link, inqmem.wire::<ScsiInquiryData>(), flags | SCSI_SILENT) {
            break 'bad Err(if lun == 0 { EINVAL } else { e });
        }
        let inqbytes = (SID_SCSI2_HDRLEN
            + usize::from(inqmem.wire::<ScsiInquiryData>().additional_length))
        .min(size_of::<ScsiInquiryData>());
        let mut inqbuf = ScsiInquiryData::new();
        inqbuf.as_bytes_mut()[..inqbytes].copy_from_slice(&inqmem.bytes()[..inqbytes]);
        drop(inqmem);
        if inqbytes < offset_of!(ScsiInquiryData, vendor) {
            inqbuf.vendor.fill(b' ');
        }
        if inqbytes < offset_of!(ScsiInquiryData, product) {
            inqbuf.product.fill(b' ');
        }
        if inqbytes < offset_of!(ScsiInquiryData, revision) {
            inqbuf.revision.fill(b' ');
        }
        if inqbytes < offset_of!(ScsiInquiryData, extra) {
            inqbuf.extra.fill(b' ');
        }
        link.inqdata.set(inqbuf);

        let nodevice = inqbuf.device & SID_TYPE == T_NODEVICE;
        match inqbuf.device & SID_QUAL {
            SID_QUAL_RSVD | SID_QUAL_BAD_LU => break 'bad Ok(()),
            SID_QUAL_LU_OFFLINE => {
                if !(lun == 0 && nodevice) {
                    break 'bad Ok(());
                }
            }
            _ => {
                // SID_QUAL_LU_OK and the vendor-specific qualifiers.
                if nodevice {
                    break 'bad Ok(());
                }
            }
        }

        scsi_devid(link);

        let link0 = scsi_get_link(sb, target, 0);
        if let Some(link0) = link0
            && lun != 0
            && link.flags.get() & SDEV_UMASS == 0
            // SAFETY: both ids, when set, are `devid_alloc` allocations their links hold.
            && (link.id.get().is_none() || unsafe { devid_cmp(link0.id(), link.id()) })
            && dumbscan == 1
            && inqbuf == link0.inqdata.get()
        {
            // The device doesn't distinguish between LUNs.
            // SC_DEBUG(link, SDEV_DB1, ("IDENTIFY not supported.\n")).
            break 'bad Err(EINVAL);
        }

        link.quirks.set(devquirks); // Restore what the device wanted.

        let (finger, _priority) = scsi_inqmatch(&inqbuf, &SCSI_QUIRK_PATTERNS);
        if let Some(finger) = finger {
            link.quirks.set(link.quirks.get() | finger.quirks);
        }

        let mut quirks = link.quirks.get();
        match sid_ansii_rev(&inqbuf) {
            SCSI_REV_0 | SCSI_REV_1 => {
                quirks |= SDEV_NOTAGS | SDEV_NOSYNC | SDEV_NOWIDE | SDEV_NOSYNCCACHE;
            }
            SCSI_REV_2 | SCSI_REV_SPC | SCSI_REV_SPC2 => {
                if inqbuf.flags & SID_CmdQue == 0 {
                    quirks |= SDEV_NOTAGS;
                }
                if inqbuf.flags & SID_Sync == 0 {
                    quirks |= SDEV_NOSYNC;
                }
                if inqbuf.flags & SID_WBus16 == 0 {
                    quirks |= SDEV_NOWIDE;
                }
            }
            // By this time SID_Sync and SID_WBus16 were obsolete.
            SCSI_REV_SPC3 | SCSI_REV_SPC4 | SCSI_REV_SPC5 if inqbuf.flags & SID_CmdQue == 0 => {
                quirks |= SDEV_NOTAGS;
            }
            _ => {}
        }
        link.quirks.set(quirks);

        // If the device can't use tags, >1 opening may confuse it.
        if quirks & SDEV_NOTAGS != 0 {
            link.openings.set(1);
        }

        // note what BASIC type of device it is
        if inqbuf.dev_qual2 & SID_REMOVABLE != 0 {
            link.flags.set(link.flags.get() | SDEV_REMOVABLE);
        }

        let mut sa = ScsiAttachArgs { sa_sc_link: link };
        let aux: *mut c_void = ptr::from_mut(&mut sa).cast();

        let Some(cf) = config_search(Some(scsibussubmatch), &sb.sc_dev, aux) else {
            scsibussubprint(aux, Some(sb.sc_dev.xname().as_bytes()));
            kprintf!(" not configured\n");
            break 'bad Ok(());
        };

        // Braindead USB devices, especially some x-in-1 media readers, try to 'help' by
        // pretending any LUN is actually LUN 0 until they see a different LUN used in a
        // command. So do an INQUIRY on LUN 1 at this point to prevent such helpfulness
        // before it causes confusion.
        if lun == 0
            && link.flags.get() & SDEV_UMASS != 0
            && scsi_get_link(sb, target, 1).is_none()
            && sb.sb_luns.get() > 1
            && let Some(mut usbinqbuf) = DmaBuf::new(size_of::<ScsiInquiryData>(), M_NOWAIT)
        {
            link.lun.set(1);
            let _ = scsi_inquire(
                link,
                usbinqbuf.wire::<ScsiInquiryData>(),
                flags | SCSI_SILENT,
            );
            link.lun.set(0);
        }

        scsi_add_link(link);

        // Generate a TEST_UNIT_READY command. This gives drivers waiting for valid quirks
        // data a chance to set wide/sync/tag options appropriately. It also clears any
        // outstanding ACA conditions that INQUIRY may leave behind.
        //
        // Do this now so that any messages generated by config_attach() do not have
        // negotiation messages inserted into their midst.
        let _ = scsi_test_unit_ready(
            link,
            TEST_READY_RETRIES,
            flags | SCSI_IGNORE_ILLEGAL_REQUEST | SCSI_IGNORE_NOT_READY | SCSI_IGNORE_MEDIA_CHANGE,
        );

        config_attach(Some(&sb.sc_dev), cf, aux, Some(scsibussubprint));
        return Ok(());
    };

    // bad:
    // SAFETY: the link was made above and is on no list but possibly the bus's (it is not
    // here); no driver has it, so nothing uses it after it is freed.
    let _ = unsafe { scsi_detach_link(link, DETACH_FORCE) };
    rslt
}

/// `scsi_detach`: detaches the devices of the bus, of a target or of one LUN (-1: all).
pub fn scsi_detach(
    sb: &'static ScsibusSoftc,
    target: i32,
    lun: i32,
    flags: i32,
) -> Result<(), Errno> {
    if target == -1 && lun == -1 {
        scsi_detach_bus(sb, flags)
    } else if lun == -1 {
        scsi_detach_target(sb, target, flags)
    } else {
        scsi_detach_lun(sb, target, lun, flags)
    }
}

/// `scsi_detach_bus`: detaches all links from the bus.
fn scsi_detach_bus(sb: &'static ScsibusSoftc, flags: i32) -> Result<(), Errno> {
    let mut rv = Ok(());
    for link in sb.sc_link_list.iter() {
        // SAFETY: the bus's links were made by `scsi_probe_link`; their devices detach
        // first, and the iterator has read the next link before this one is freed.
        let r = unsafe { scsi_detach_link(link, flags) };
        if matches!(r, Err(e) if e != ENXIO) {
            rv = r;
        }
    }
    rv
}

/// `scsi_detach_target`: detaches all links from the target.
pub fn scsi_detach_target(sb: &'static ScsibusSoftc, target: i32, flags: i32) -> Result<(), Errno> {
    let mut rv = Ok(());
    for link in sb.sc_link_list.iter() {
        if i32::from(link.target.get()) == target {
            // SAFETY: as in `scsi_detach_bus`.
            let r = unsafe { scsi_detach_link(link, flags) };
            if matches!(r, Err(e) if e != ENXIO) {
                rv = r;
            }
        }
    }
    rv
}

/// `scsi_detach_lun`: detaches the (target, lun) link; `Err(EINVAL)` when there is none.
pub fn scsi_detach_lun(
    sb: &'static ScsibusSoftc,
    target: i32,
    lun: i32,
    flags: i32,
) -> Result<(), Errno> {
    let link = scsi_get_link(sb, target, lun).ok_or(EINVAL)?;

    // SAFETY: as in `scsi_detach_bus`.
    unsafe { scsi_detach_link(link, flags) }
}

/// `scsi_detach_link`: detaching a device from scsibus is a five step process; on success
/// the link is freed. `Err(EBUSY)` for an open device without `DETACH_FORCE`.
///
/// # Safety
///
/// `link` was made by `scsi_probe_link`. On success it is freed: neither the caller nor
/// anyone else (its device has detached by then) may use it again.
unsafe fn scsi_detach_link(link: &'static ScsiLink, flags: i32) -> Result<(), Errno> {
    let sb = link.bus();

    if flags & DETACH_FORCE == 0 && link.flags.get() & SDEV_OPEN != 0 {
        return Err(EBUSY);
    }

    // 1. Wake up processes sleeping for an xs.
    if link.pool.get().is_some() {
        scsi_link_shutdown(link);
    }

    // 2. Detach the device.
    if let Some(dev) = link.device_softc.get() {
        // SAFETY: the driver's attached device; nothing here uses it afterwards.
        unsafe { config_detach(dev, flags)? };
    }

    // 3. If it's using the openings io allocator, clean that up.
    if let Some(pool) = link.pool.get()
        && link.flags.get() & SDEV_OWN_IOPL != 0
    {
        scsi_iopool_destroy(pool);
        free(
            NonNull::from(pool).cast(),
            M_DEVBUF,
            size_of::<ScsiIopool>(),
        );
    }

    // 4. Free up its state in the adapter.
    if let Some(dev_free) = sb.adapter().dev_free {
        dev_free(link);
    }

    // 5. Free up its state in the midlayer.
    if let Some(id) = link.id.get() {
        // SAFETY: the link's reference to a `devid_alloc` allocation, given up here.
        unsafe { devid_free(id) };
    }
    scsi_remove_link(link);
    free(NonNull::from(link).cast(), M_DEVBUF, size_of::<ScsiLink>());

    Ok(())
}

/// `scsi_get_link`: the link of (target, lun) on the bus, if any.
pub fn scsi_get_link(
    sb: &'static ScsibusSoftc,
    target: i32,
    lun: i32,
) -> Option<&'static ScsiLink> {
    sb.sc_link_list
        .iter()
        .find(|link| i32::from(link.target.get()) == target && i32::from(link.lun.get()) == lun)
}

/// `scsi_add_link`: puts the link on its bus's list.
fn scsi_add_link(link: &'static ScsiLink) {
    // SAFETY: a link is added once, after its probe, and stays in place until
    // `scsi_remove_link` unlinks it before it is freed.
    unsafe { link.bus().sc_link_list.insert_head(link) };
}

/// `scsi_remove_link`: takes the link off its bus's list, if it is there.
fn scsi_remove_link(link: &ScsiLink) {
    let sb = link.bus();
    let mut prev: Option<&ScsiLink> = None;

    for elm in sb.sc_link_list.iter() {
        if ptr::eq(elm, link) {
            match prev {
                // SAFETY: `link` is the first element.
                None => unsafe { sb.sc_link_list.remove_head() },
                // SAFETY: `prev` is linked and `link` follows it.
                Some(prev) => unsafe { SlistHead::<ScsiLinkBusList>::remove_after(prev) },
            }
            break;
        }
        prev = Some(elm);
    }
}

/// `scsi_get_target_luns`: the LUNs to probe on the target: the type 0 LUNs REPORT LUNS
/// lists, or all `sb_luns` of them (`dumbscan`) when the device cannot say, and none when
/// there is no LUN 0.
fn scsi_get_target_luns(sb: &'static ScsibusSoftc, target: i32) -> ScsiLunArray {
    let mut lunarray = ScsiLunArray {
        luns: [0; 256],
        count: 0,
        dumbscan: 0,
    };

    // LUN 0 *must* be present.
    let _ = scsi_probe_link(sb, target, 0, 0);
    let Some(link0) = scsi_get_link(sb, target, 0) else {
        return lunarray;
    };

    // Initialize dumbscan result. Just in case.
    let nluns = link0.bus().sb_luns.get();
    for i in 0..nluns {
        lunarray.luns[usize::from(i)] = i;
    }
    lunarray.count = i32::from(nluns);
    lunarray.dumbscan = 1;

    // ATAPI, USB and pre-SPC (i.e. pre-SCSI-3) devices can't ask for a report of valid
    // LUNs.
    if link0.flags.get() & (SDEV_UMASS | SDEV_ATAPI) != 0
        || sid_ansii_rev(&link0.inqdata.get()) < SCSI_REV_SPC
    {
        return lunarray;
    }

    let Some(mut report) = DmaBuf::new(size_of::<ScsiReportLunsData>(), M_WAITOK) else {
        return lunarray;
    };

    if scsi_report_luns(
        link0,
        REPORT_NORMAL,
        report.wire::<ScsiReportLunsData>(),
        size_of::<ScsiReportLunsData>() as u32,
        scsi_autoconf.load(Ordering::Relaxed)
            | SCSI_SILENT
            | SCSI_IGNORE_ILLEGAL_REQUEST
            | SCSI_IGNORE_NOT_READY
            | SCSI_IGNORE_MEDIA_CHANGE,
        10000,
    )
    .is_err()
    {
        return lunarray;
    }

    // XXX In theory we should check if data is full, which would indicate it needs to be
    // enlarged and REPORT LUNS tried again. Solaris tries up to 3 times with larger sizes
    // for data.

    // Return the reported Type-0 LUNs. Type-0 only!
    let report = report.wire::<ScsiReportLunsData>();
    lunarray.count = 0;
    lunarray.dumbscan = 0;
    let nluns = (_4btol(&report.length) as usize / RPL_LUNDATA_SIZE).min(report.luns.len());
    for lun in &report.luns[..nluns] {
        if lun.lundata[0] != 0 {
            continue;
        }
        lunarray.luns[lunarray.count as usize] = lun.lundata[RPL_LUNDATA_T0LUN];
        lunarray.count += 1;
    }

    lunarray
}

/// `scsi_strvis`: `src` made printable into `dst`, NUL-terminated: leading and trailing
/// whitespace and NULs trimmed, inner runs of them collapsed to one space, backslashes
/// doubled, other unprintable bytes as `\ooo`. `dst` needs `4 * src.len() + 1` bytes; what
/// does not fit is dropped.
pub fn scsi_strvis(dst: &mut [u8], src: &[u8]) {
    let blank = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\0' | 0xff);

    // Trim leading and trailing whitespace and NULs.
    let start = src.iter().position(|&c| !blank(c)).unwrap_or(src.len());
    let end = src
        .iter()
        .rposition(|&c| !blank(c))
        .map_or(start, |i| i + 1);
    let src = &src[start..end];

    let Some(room) = dst.len().checked_sub(1) else {
        return;
    };
    let mut n = 0;
    let mut put = |c: u8| {
        if n < room {
            dst[n] = c;
            n += 1;
        }
    };

    let mut last = 0xff;
    for &c in src {
        if blank(c) {
            // Collapse whitespace and NULs to a single space.
            if last != b' ' {
                put(b' ');
            }
            last = b' ';
        } else if c == b'\\' {
            // Quote backslashes.
            put(b'\\');
            put(b'\\');
            last = b'\\';
        } else {
            if !(0x20..0x80).contains(&c) {
                // Non-printable characters to octal.
                put(b'\\');
                put(((c & 0o300) >> 6) + b'0');
                put(((c & 0o070) >> 3) + b'0');
                put((c & 0o007) + b'0');
            } else {
                // Copy normal characters.
                put(c);
            }
            last = c;
        }
    }

    dst[n] = 0;
}

/// `scsi_print_link`: ` targ T lun L: <vendor, product, revision>`, then removable and the
/// device identifier.
fn scsi_print_link(link: &ScsiLink) {
    let mut visbuf = [0u8; 65];
    let inqbuf = link.inqdata.get();

    kprintf!(" targ {} lun {}: ", link.target.get(), link.lun.get());

    scsi_strvis(&mut visbuf, &inqbuf.vendor);
    kprintf!("<{}, ", Str(&visbuf));
    scsi_strvis(&mut visbuf, &inqbuf.product);
    kprintf!("{}, ", Str(&visbuf));
    scsi_strvis(&mut visbuf, &inqbuf.revision);
    kprintf!("{}>", Str(&visbuf));

    // SCSIDEBUG (not configured): " ATAPI", " SCSI/%d", " SCSI/SPC" or " SCSI/SPC-%d".

    if link.flags.get() & SDEV_REMOVABLE != 0 {
        kprintf!(" removable");
    }

    if let Some(d) = link.id()
        && d.d_type != DEVID_NONE
    {
        // SAFETY: the link's id is a `devid_alloc` allocation.
        let id = unsafe { d.id() };
        match d.d_type {
            DEVID_NAA => kprintf!(" naa."),
            DEVID_EUI => kprintf!(" eui."),
            DEVID_T10 => kprintf!(" t10."),
            DEVID_SERIAL => kprintf!(" serial."),
            DEVID_WWN => kprintf!(" wwn."),
            _ => 0,
        };

        if d.d_flags & DEVID_F_PRINT != 0 {
            for (i, &c) in id.iter().enumerate() {
                if c == b'\0' || c == b' ' {
                    // skip leading blanks
                    // collapse multiple blanks into one
                    if i > 0 && id[i - 1] != c {
                        kprintf!("_");
                    }
                } else if !(0x20..0x80).contains(&c) {
                    // non-printable characters
                    kprintf!("~");
                } else {
                    // normal characters
                    kprintf!("{}", char::from(c));
                }
            }
        } else {
            for c in id {
                kprintf!("{:02x}", c);
            }
        }
    }
    // SCSIDEBUG (not configured): the link's state, luns, openings, flags and quirks.
}

/// `scsi_inqmatch`: the pattern of `base` that best matches the inquiry data, and its
/// priority (2 for type and removability, plus the length of each vendor, product and
/// revision prefix that matched; 0 and `None` when nothing does). An earlier pattern wins
/// a tie.
///
/// `base` is any table of patterns, or of structures that begin with one (the C passes the
/// element size; here `AsRef` finds the pattern).
pub fn scsi_inqmatch<'a, T: AsRef<ScsiInquiryPattern>>(
    inqbuf: &ScsiInquiryData,
    base: &'a [T],
) -> (Option<&'a T>, i32) {
    // Include the qualifier to catch vendor-unique types.
    let removable = if inqbuf.dev_qual2 & SID_REMOVABLE != 0 {
        T_REMOV
    } else {
        T_FIXED
    };
    let bytes = inqbuf.as_bytes();
    // `bcmp(inqbuf->vendor, match->vendor, strlen(match->vendor))`: a long pattern goes on
    // into the fields that follow, as in C.
    let matches = |off: usize, pat: &[u8]| bytes.get(off..off + pat.len()) == Some(pat);

    let mut bestpriority = 0;
    let mut bestmatch = None;
    for entry in base {
        let pattern = entry.as_ref();

        if inqbuf.device != pattern.r#type {
            continue;
        }
        if removable != pattern.removable {
            continue;
        }
        if !matches(offset_of!(ScsiInquiryData, vendor), pattern.vendor)
            || !matches(offset_of!(ScsiInquiryData, product), pattern.product)
            || !matches(offset_of!(ScsiInquiryData, revision), pattern.revision)
        {
            continue;
        }
        let priority =
            2 + (pattern.vendor.len() + pattern.product.len() + pattern.revision.len()) as i32;

        // SCSIDEBUG (not configured): prints the match, and the quirks of a quirk entry.

        if priority > bestpriority {
            bestpriority = priority;
            bestmatch = Some(entry);
        }
    }

    (bestmatch, bestpriority)
}

/// `scsi_devid`: finds the link's device identifier: from VPD page 0x83, else page 0x80,
/// else the node's world wide name.
fn scsi_devid(link: &'static ScsiLink) {
    /// `struct { struct scsi_vpd_hdr hdr; u_int8_t list[32]; } __packed`.
    const PG_LIST: usize = 32;

    if link.id.get().is_some() {
        return;
    }

    'wwn: {
        let Some(mut pg) = DmaBuf::new(size_of::<ScsiVpdHdr>() + PG_LIST, M_WAITOK) else {
            break 'wwn;
        };

        if sid_ansii_rev(&link.inqdata.get()) >= SCSI_REV_2 {
            let bytes = pg.bytes();
            if scsi_inquire_vpd(
                link,
                bytes,
                SI_PG_SUPPORTED,
                scsi_autoconf.load(Ordering::Relaxed),
            )
            .is_err()
            {
                break 'wwn;
            }

            let hdr = wire_ref::<ScsiVpdHdr>(bytes);
            let len = PG_LIST.min(_2btol(&hdr.page_length) as usize);
            let list = &bytes[size_of::<ScsiVpdHdr>()..][..len];
            let pg80 = list.contains(&SI_PG_SERIAL);
            let pg83 = list.contains(&SI_PG_DEVID);

            if pg83 && scsi_devid_pg83(link).is_ok() {
                return;
            }
            if pg80 && scsi_devid_pg80(link).is_ok() {
                return;
            }
        }
    }

    // wwn:
    let _ = scsi_devid_wwn(link);
}

/// `scsi_devid_pg83`: the logical unit's best designator in the device identification
/// page (NAA over EUI-64 over T10).
fn scsi_devid_pg83(link: &'static ScsiLink) -> Result<(), Errno> {
    let flags = scsi_autoconf.load(Ordering::Relaxed);
    let hdrlen = size_of::<ScsiVpdHdr>();
    let dhdrlen = size_of::<ScsiVpdDevidHdr>();

    let mut hdr = DmaBuf::new(hdrlen, M_WAITOK).ok_or(ENOMEM)?;
    scsi_inquire_vpd(link, hdr.bytes(), SI_PG_DEVID, flags)?;

    let len = hdrlen + _2btol(&hdr.wire::<ScsiVpdHdr>().page_length) as usize;
    let mut pgbuf = DmaBuf::new(len, M_WAITOK).ok_or(ENOMEM)?;
    scsi_inquire_vpd(link, pgbuf.bytes(), SI_PG_DEVID, flags)?;
    let pg = pgbuf.bytes();

    let mut pos = hdrlen;
    let mut idtype = 0;
    let mut chosen: Option<(ScsiVpdDevidHdr, usize)> = None;

    loop {
        if len - pos < dhdrlen {
            return Err(EIO);
        }
        let dhdr = ScsiVpdDevidHdr::read_from(&pg[pos..]);
        pos += dhdrlen;
        if len - pos < usize::from(dhdr.len) {
            return Err(EIO);
        }

        if vpd_devid_assoc(dhdr.flags) == VPD_DEVID_ASSOC_LU {
            let r#type = vpd_devid_type(dhdr.flags);
            if matches!(
                r#type,
                VPD_DEVID_TYPE_NAA | VPD_DEVID_TYPE_EUI64 | VPD_DEVID_TYPE_T10
            ) && r#type >= idtype
            {
                idtype = r#type;
                chosen = Some((dhdr, pos));
            }
            // Other types: skip.
        }

        pos += usize::from(dhdr.len);
        if idtype == VPD_DEVID_TYPE_NAA || len == pos {
            break;
        }
    }

    let Some((chdr, id)) = chosen else {
        return Err(ENODEV);
    };
    let d_type = match vpd_devid_type(chdr.flags) {
        VPD_DEVID_TYPE_NAA => DEVID_NAA,
        VPD_DEVID_TYPE_EUI64 => DEVID_EUI,
        _ => DEVID_T10, // VPD_DEVID_TYPE_T10, the only other type chosen
    };
    let idflags = match vpd_devid_code(chdr.pi_code) {
        VPD_DEVID_CODE_ASCII | VPD_DEVID_CODE_UTF8 => DEVID_F_PRINT,
        _ => 0,
    };
    link.id.set(devid_alloc(
        d_type,
        idflags,
        chdr.len,
        &pg[id..id + usize::from(chdr.len)],
    ));

    Ok(())
}

/// `scsi_devid_pg80`: vendor, product and the unit serial number page, as a serial
/// identifier.
fn scsi_devid_pg80(link: &'static ScsiLink) -> Result<(), Errno> {
    let flags = scsi_autoconf.load(Ordering::Relaxed);
    let hdrlen = size_of::<ScsiVpdHdr>();

    let mut hdr = DmaBuf::new(hdrlen, M_WAITOK).ok_or(ENOMEM)?;
    scsi_inquire_vpd(link, hdr.bytes(), SI_PG_SERIAL, flags)?;

    let len = _2btol(&hdr.wire::<ScsiVpdHdr>().page_length) as usize;
    if len == 0 {
        return Err(EINVAL);
    }

    let pglen = hdrlen + len;
    let mut pg = DmaBuf::new(pglen, M_WAITOK).ok_or(ENOMEM)?;
    scsi_inquire_vpd(link, pg.bytes(), SI_PG_SERIAL, flags)?;

    let inqdata = link.inqdata.get();
    let (vlen, plen) = (inqdata.vendor.len(), inqdata.product.len());
    let idlen = vlen + plen + len;
    let mut id = DmaBuf::new(idlen, M_WAITOK).ok_or(ENOMEM)?;
    let idb = id.bytes();
    idb[..vlen].copy_from_slice(&inqdata.vendor);
    idb[vlen..vlen + plen].copy_from_slice(&inqdata.product);
    idb[vlen + plen..].copy_from_slice(&pg.bytes()[hdrlen..]);

    // devid_alloc's length is a u_int8_t: the C passes the sum truncated.
    link.id
        .set(devid_alloc(DEVID_SERIAL, DEVID_F_PRINT, idlen as u8, idb));

    Ok(())
}

/// `scsi_devid_wwn`: the node's world wide name, for LUN 0; `Err(EOPNOTSUPP)` without one.
fn scsi_devid_wwn(link: &ScsiLink) -> Result<(), Errno> {
    if link.lun.get() != 0 || link.node_wwn.get() == 0 {
        return Err(EOPNOTSUPP);
    }

    let wwnn = link.node_wwn.get().to_be_bytes();
    link.id
        .set(devid_alloc(DEVID_WWN, 0, wwnn.len() as u8, &wwnn));

    Ok(())
}

/// `devid_alloc`: a device identifier of `len` bytes from `id`, with one reference; `None`
/// when the memory is not there (`M_CANFAIL`). Bytes `id` lacks are zero.
pub fn devid_alloc(r#type: u8, flags: u8, len: u8, id: &[u8]) -> Option<NonNull<Devid>> {
    let len_bytes = usize::from(len);
    let mem = malloc(
        size_of::<Devid>() + len_bytes,
        M_DEVBUF,
        M_WAITOK | M_CANFAIL | M_ZERO,
    )?;
    let d = mem.cast::<Devid>();
    let src = &id[..len_bytes.min(id.len())];

    // SAFETY: a fresh allocation of the header plus `len` bytes, aligned by malloc(9); the
    // header is written once and `src` (at most `len` bytes) lands right after it.
    unsafe {
        d.as_ptr().write(Devid {
            d_type: r#type,
            d_flags: flags,
            d_refcount: Cell::new(1),
            d_len: len,
        });
        ptr::copy_nonoverlapping(src.as_ptr(), d.as_ptr().add(1).cast::<u8>(), src.len());
    }

    Some(d)
}

/// `devid_copy`: one more reference to `d`.
pub fn devid_copy(d: &Devid) -> NonNull<Devid> {
    d.d_refcount.set(d.d_refcount.get().wrapping_add(1));
    NonNull::from(d)
}

/// `devid_free`: drops a reference to `d`, freeing it with the last one.
///
/// # Safety
///
/// `d` is a `devid_alloc` allocation the caller holds a reference to, which it gives up:
/// after the last reference nobody may use `d`.
pub unsafe fn devid_free(d: NonNull<Devid>) {
    // SAFETY: the caller's reference keeps the allocation alive here.
    let dref = unsafe { d.as_ref() };
    let refs = dref.d_refcount.get().wrapping_sub(1);
    dref.d_refcount.set(refs);
    if refs == 0 {
        free(
            d.cast(),
            M_DEVBUF,
            size_of::<Devid>() + usize::from(dref.d_len),
        );
    }
}

#[cfg(test)]
mod tests;
