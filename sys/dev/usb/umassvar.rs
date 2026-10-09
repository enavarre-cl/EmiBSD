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
 * Copyright (c) 1999 MAEKAWA Masahide <bishop@rr.iij4u.or.jp>,
 *		      Nick Hibma <n_hibma@freebsd.org>
 * All rights reserved.
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
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *     $FreeBSD: src/sys/dev/usb/umass.c,v 1.13 2000/03/26 01:39:12 n_hibma Exp $
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/usb/umassvar.h>`: the umass(4) softc, the Bulk-Only and CBI wire structures, and
//! the transfer states.
//!
//! Upstream: sys/dev/usb/umassvar.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `UMASS_DEBUG` is not in GENERIC: `DIF`, `DPRINTF`, the `UDMASS_*` masks, `umassdebug`
//!   and the softc's `tv` are absent, as in a kernel built without it.
//! - `umass_cbi_sbl_t`, a union of two two-byte structures, is [`UmassCbiSbl`]: the two
//!   bytes with accessors for each view (`common.type`/`common.value`, `ufi.asc`/
//!   `ufi.ascq`).
//! - The softc's members the stack changes after attach are `Cell`s (under the kernel lock
//!   at `splusb()`, as everything in USB but a host controller's hard interrupt);
//!   `sc_udev`, `sc_iface`, `sc_pipe[]`, `transfer_xfer[]` and `next_polled_xfer` are
//!   `Option<&'static T>` (`docs/C_TO_RUST.md`, `usbd_device *`), `bus` an
//!   `Option<NonNull<UmassScsiSoftc>>` (allocated and freed by `umass_scsi.c`).
//! - `umass_wire_xfer` takes the command as one slice (the C's `cmd`, `cmdlen`) and is an
//!   `unsafe fn`: `data` is a raw buffer kept until the callback runs.

use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::NonNull;

use crate::dev::usb::umass_scsi::UmassScsiSoftc;
use crate::dev::usb::usb::{UByte, UDWord, UsbDeviceRequest, UsbWire};
use crate::dev::usb::usbdi::{UsbdCallback, UsbdStatus};
use crate::dev::usb::usbdivar::{UsbdDevice, UsbdInterface, UsbdPipe, UsbdXfer};
use crate::kern::subr_prf::panic;
use crate::sys::device::{Device, Softc};
use crate::sys::param::MAXBSIZE;

/* Generic definitions */

/// `UFI_COMMAND_LENGTH`.
pub const UFI_COMMAND_LENGTH: u8 = 12;

/* Direction for umass_*_transfer */
/// `DIR_NONE`.
pub const DIR_NONE: i32 = 0;
/// `DIR_IN`.
pub const DIR_IN: i32 = 1;
/// `DIR_OUT`.
pub const DIR_OUT: i32 = 2;

/* Endpoints for umass */
/// `UMASS_BULKIN`.
pub const UMASS_BULKIN: usize = 0;
/// `UMASS_BULKOUT`.
pub const UMASS_BULKOUT: usize = 1;
/// `UMASS_INTRIN`.
pub const UMASS_INTRIN: usize = 2;
/// `UMASS_NEP`.
pub const UMASS_NEP: usize = 3;

/* Bulk-Only features */

/// `UR_BBB_RESET`: Bulk-Only reset.
pub const UR_BBB_RESET: u8 = 0xff;
/// `UR_BBB_GET_MAX_LUN`.
pub const UR_BBB_GET_MAX_LUN: u8 = 0xfe;

/// `CBWSIGNATURE`.
pub const CBWSIGNATURE: u32 = 0x4342_5355;
/// `CBWFLAGS_OUT`.
pub const CBWFLAGS_OUT: u8 = 0x00;
/// `CBWFLAGS_IN`.
pub const CBWFLAGS_IN: u8 = 0x80;
/// `CBWCDBLENGTH`.
pub const CBWCDBLENGTH: usize = 16;

/// `struct umass_bbb_cbw`: Command Block Wrapper.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
#[allow(non_snake_case)] // the specification's member names, as in C
pub struct UmassBbbCbw {
    /// `dCBWSignature`: `CBWSIGNATURE`.
    pub dCBWSignature: UDWord,
    /// `dCBWTag`.
    pub dCBWTag: UDWord,
    /// `dCBWDataTransferLength`.
    pub dCBWDataTransferLength: UDWord,
    /// `bCBWFlags`: `CBWFLAGS_*`.
    pub bCBWFlags: UByte,
    /// `bCBWLUN`.
    pub bCBWLUN: UByte,
    /// `bCDBLength`.
    pub bCDBLength: UByte,
    /// `CBWCDB`.
    pub CBWCDB: [UByte; CBWCDBLENGTH],
}

// SAFETY: `#[repr(C)]` of `u8`s and arrays of them.
unsafe impl UsbWire for UmassBbbCbw {}

/// `UMASS_BBB_CBW_SIZE`.
pub const UMASS_BBB_CBW_SIZE: u32 = 31;

/// `CSWSIGNATURE`.
pub const CSWSIGNATURE: u32 = 0x5342_5355;
/// `CSWSIGNATURE_OLYMPUS_C1`.
pub const CSWSIGNATURE_OLYMPUS_C1: u32 = 0x5542_5355;
/// `CSWSTATUS_GOOD`.
pub const CSWSTATUS_GOOD: u8 = 0x0;
/// `CSWSTATUS_FAILED`.
pub const CSWSTATUS_FAILED: u8 = 0x1;
/// `CSWSTATUS_PHASE`.
pub const CSWSTATUS_PHASE: u8 = 0x2;

/// `struct umass_bbb_csw`: Command Status Wrapper.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
#[allow(non_snake_case)] // the specification's member names, as in C
pub struct UmassBbbCsw {
    /// `dCSWSignature`: `CSWSIGNATURE`.
    pub dCSWSignature: UDWord,
    /// `dCSWTag`.
    pub dCSWTag: UDWord,
    /// `dCSWDataResidue`.
    pub dCSWDataResidue: UDWord,
    /// `bCSWStatus`: `CSWSTATUS_*`.
    pub bCSWStatus: UByte,
}

// SAFETY: `#[repr(C)]` of `u8`s and arrays of them.
unsafe impl UsbWire for UmassBbbCsw {}

/// `UMASS_BBB_CSW_SIZE`.
pub const UMASS_BBB_CSW_SIZE: u32 = 13;

/* CBI features */

/// `UR_CBI_ADSC`.
pub const UR_CBI_ADSC: u8 = 0x00;

/// `umass_cbi_cbl_t`: command block.
pub type UmassCbiCbl = [u8; 16];

/// `IDB_TYPE_CCI` (`common.type`).
pub const IDB_TYPE_CCI: u8 = 0x00;
/// `IDB_VALUE_PASS` (`common.value`).
pub const IDB_VALUE_PASS: u8 = 0x00;
/// `IDB_VALUE_FAIL`.
pub const IDB_VALUE_FAIL: u8 = 0x01;
/// `IDB_VALUE_PHASE`.
pub const IDB_VALUE_PHASE: u8 = 0x02;
/// `IDB_VALUE_PERSISTENT`.
pub const IDB_VALUE_PERSISTENT: u8 = 0x03;
/// `IDB_VALUE_STATUS_MASK`.
pub const IDB_VALUE_STATUS_MASK: u8 = 0x03;

/// `umass_cbi_sbl_t`: the status block of the interrupt endpoint, read either as
/// `common` (`type`, `value`) or as `ufi` (`asc`, `ascq`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UmassCbiSbl {
    /// The two bytes of the union.
    pub bytes: [UByte; 2],
}

impl UmassCbiSbl {
    /// `sbl.common.type`.
    pub fn common_type(&self) -> u8 {
        self.bytes[0]
    }

    /// `sbl.common.value`.
    pub fn common_value(&self) -> u8 {
        self.bytes[1]
    }

    /// `sbl.ufi.asc`.
    pub fn ufi_asc(&self) -> u8 {
        self.bytes[0]
    }

    /// `sbl.ufi.ascq`.
    pub fn ufi_ascq(&self) -> u8 {
        self.bytes[1]
    }
}

// SAFETY: `#[repr(C)]` of an array of `u8`.
unsafe impl UsbWire for UmassCbiSbl {}

/// `umass_callback`: the end of a command (`residue` bytes not transferred, `status` one of
/// `STATUS_*`).
pub type UmassCallback = fn(sc: &'static UmassSoftc, priv_: *mut c_void, residue: i32, status: i32);

/// `STATUS_CMD_OK`: everything ok.
pub const STATUS_CMD_OK: i32 = 0;
/// `STATUS_CMD_UNKNOWN`: will have to fetch sense.
pub const STATUS_CMD_UNKNOWN: i32 = 1;
/// `STATUS_CMD_FAILED`: transfer was ok, command failed.
pub const STATUS_CMD_FAILED: i32 = 2;
/// `STATUS_WIRE_FAILED`: couldn't even get command across.
pub const STATUS_WIRE_FAILED: i32 = 3;

/// `umass_wire_xfer`: sends the command `cmd` to `lun`, moves `datalen` bytes at `data` in
/// direction `dir` (`DIR_*`), and calls `cb(sc, priv_, residue, status)` at the end.
///
/// # Safety
///
/// Unless `datalen` is 0, `data` is valid for `datalen` bytes (reads for `DIR_OUT`, writes
/// for `DIR_IN`), at most `UMASS_MAX_TRANSFER_SIZE`, until `cb` has been called; `priv_` is
/// what `cb` expects.
pub type UmassWireXfer = unsafe fn(
    sc: &'static UmassSoftc,
    lun: i32,
    cmd: &[u8],
    data: *mut u8,
    datalen: i32,
    dir: i32,
    timeout: u32,
    cb: UmassCallback,
    priv_: *mut c_void,
);

/// `umass_wire_reset`: starts the reset recovery, ending with `status`.
pub type UmassWireReset = fn(sc: &'static UmassSoftc, status: i32);

/// `umass_wire_state`: the transfer state machine, every xfer's callback.
pub type UmassWireState = UsbdCallback;

/// `struct umass_wire_methods`.
pub struct UmassWireMethods {
    /// `wire_xfer`.
    pub wire_xfer: UmassWireXfer,
    /// `wire_reset`.
    pub wire_reset: UmassWireReset,
    /// `wire_state`.
    pub wire_state: UmassWireState,
}

/// `UMASS_WPROTO_UNSPEC` (`sc_wire`: wire protocol).
pub const UMASS_WPROTO_UNSPEC: u8 = 0;
/// `UMASS_WPROTO_BBB`.
pub const UMASS_WPROTO_BBB: u8 = 1;
/// `UMASS_WPROTO_CBI`.
pub const UMASS_WPROTO_CBI: u8 = 2;
/// `UMASS_WPROTO_CBI_I`.
pub const UMASS_WPROTO_CBI_I: u8 = 3;

/// `UMASS_CPROTO_UNSPEC` (`sc_cmd`: command protocol).
pub const UMASS_CPROTO_UNSPEC: u8 = 0;
/// `UMASS_CPROTO_SCSI`.
pub const UMASS_CPROTO_SCSI: u8 = 1;
/// `UMASS_CPROTO_ATAPI`.
pub const UMASS_CPROTO_ATAPI: u8 = 2;
/// `UMASS_CPROTO_UFI`.
pub const UMASS_CPROTO_UFI: u8 = 3;
/// `UMASS_CPROTO_RBC`.
pub const UMASS_CPROTO_RBC: u8 = 4;
/// `UMASS_CPROTO_ISD_ATA`.
pub const UMASS_CPROTO_ISD_ATA: u8 = 5;

/// `UMASS_QUIRK_WRONG_CSWSIG` (`sc_quirks`).
pub const UMASS_QUIRK_WRONG_CSWSIG: u32 = 0x0000_0001;
/// `UMASS_QUIRK_WRONG_CSWTAG`.
pub const UMASS_QUIRK_WRONG_CSWTAG: u32 = 0x0000_0002;
/// `UMASS_QUIRK_IGNORE_RESIDUE`.
pub const UMASS_QUIRK_IGNORE_RESIDUE: u32 = 0x0000_0004;

/* indices into transfer_xfer[] */
/// `XFER_BBB_CBW`: Bulk-Only.
pub const XFER_BBB_CBW: usize = 0;
/// `XFER_BBB_DATA`.
pub const XFER_BBB_DATA: usize = 1;
/// `XFER_BBB_DCLEAR`.
pub const XFER_BBB_DCLEAR: usize = 2;
/// `XFER_BBB_CSW1`.
pub const XFER_BBB_CSW1: usize = 3;
/// `XFER_BBB_CSW2`.
pub const XFER_BBB_CSW2: usize = 4;
/// `XFER_BBB_SCLEAR`.
pub const XFER_BBB_SCLEAR: usize = 5;
/// `XFER_BBB_RESET1`.
pub const XFER_BBB_RESET1: usize = 6;
/// `XFER_BBB_RESET2`.
pub const XFER_BBB_RESET2: usize = 7;
/// `XFER_BBB_RESET3`.
pub const XFER_BBB_RESET3: usize = 8;

/// `XFER_CBI_CB`: CBI.
pub const XFER_CBI_CB: usize = 0;
/// `XFER_CBI_DATA`.
pub const XFER_CBI_DATA: usize = 1;
/// `XFER_CBI_STATUS`.
pub const XFER_CBI_STATUS: usize = 2;
/// `XFER_CBI_DCLEAR`.
pub const XFER_CBI_DCLEAR: usize = 3;
/// `XFER_CBI_SCLEAR`.
pub const XFER_CBI_SCLEAR: usize = 4;
/// `XFER_CBI_RESET1`.
pub const XFER_CBI_RESET1: usize = 5;
/// `XFER_CBI_RESET2`.
pub const XFER_CBI_RESET2: usize = 6;
/// `XFER_CBI_RESET3`.
pub const XFER_CBI_RESET3: usize = 7;

/// `XFER_NR`: maximum number.
pub const XFER_NR: usize = 9;

/// `TSTATE_IDLE` (`transfer_state`).
pub const TSTATE_IDLE: i32 = 0;
/// `TSTATE_BBB_COMMAND`: CBW transfer.
pub const TSTATE_BBB_COMMAND: i32 = 1;
/// `TSTATE_BBB_DATA`: Data transfer.
pub const TSTATE_BBB_DATA: i32 = 2;
/// `TSTATE_BBB_DCLEAR`: clear endpt stall.
pub const TSTATE_BBB_DCLEAR: i32 = 3;
/// `TSTATE_BBB_STATUS1`: clear endpt stall.
pub const TSTATE_BBB_STATUS1: i32 = 4;
/// `TSTATE_BBB_SCLEAR`: clear endpt stall.
pub const TSTATE_BBB_SCLEAR: i32 = 5;
/// `TSTATE_BBB_STATUS2`: CSW transfer.
pub const TSTATE_BBB_STATUS2: i32 = 6;
/// `TSTATE_BBB_RESET1`: reset command.
pub const TSTATE_BBB_RESET1: i32 = 7;
/// `TSTATE_BBB_RESET2`: in clear stall.
pub const TSTATE_BBB_RESET2: i32 = 8;
/// `TSTATE_BBB_RESET3`: out clear stall.
pub const TSTATE_BBB_RESET3: i32 = 9;
/// `TSTATE_CBI_COMMAND`: command transfer.
pub const TSTATE_CBI_COMMAND: i32 = 10;
/// `TSTATE_CBI_DATA`: data transfer.
pub const TSTATE_CBI_DATA: i32 = 11;
/// `TSTATE_CBI_STATUS`: status transfer.
pub const TSTATE_CBI_STATUS: i32 = 12;
/// `TSTATE_CBI_DCLEAR`: clear ep stall.
pub const TSTATE_CBI_DCLEAR: i32 = 13;
/// `TSTATE_CBI_SCLEAR`: clear ep stall.
pub const TSTATE_CBI_SCLEAR: i32 = 14;
/// `TSTATE_CBI_RESET1`: reset command.
pub const TSTATE_CBI_RESET1: i32 = 15;
/// `TSTATE_CBI_RESET2`: in clear stall.
pub const TSTATE_CBI_RESET2: i32 = 16;
/// `TSTATE_CBI_RESET3`: out clear stall.
pub const TSTATE_CBI_RESET3: i32 = 17;
/// `TSTATE_STATES`: # of states above.
pub const TSTATE_STATES: i32 = 18;

/// `struct umass_softc`: the per device structure.
#[repr(C)]
pub struct UmassSoftc {
    /// `sc_dev`: base device.
    pub sc_dev: Device,
    /// `sc_udev`: device.
    pub sc_udev: Cell<Option<&'static UsbdDevice>>,
    /// `sc_iface`: interface.
    pub sc_iface: Cell<Option<&'static UsbdInterface>>,
    /// `sc_ifaceno`: interface number.
    pub sc_ifaceno: Cell<i32>,

    /// `sc_epaddr`.
    pub sc_epaddr: [Cell<u8>; UMASS_NEP],
    /// `sc_pipe`.
    pub sc_pipe: [Cell<Option<&'static UsbdPipe>>; UMASS_NEP],
    /// `sc_req`.
    pub sc_req: Cell<UsbDeviceRequest>,

    /// `sc_methods`.
    pub sc_methods: Cell<Option<&'static UmassWireMethods>>,

    /// `sc_wire`: wire protocol (`UMASS_WPROTO_*`).
    pub sc_wire: Cell<u8>,
    /// `sc_cmd`: command protocol (`UMASS_CPROTO_*`).
    pub sc_cmd: Cell<u8>,
    /// `sc_quirks`: `UMASS_QUIRK_*`.
    pub sc_quirks: Cell<u32>,
    /// `sc_busquirks`: the SCSI link's quirks (`ADEV_*`, `SDEV_*`).
    pub sc_busquirks: Cell<u32>,

    /* Bulk specific variables for transfers in progress */
    /// `cbw`: command block wrapper.
    pub cbw: Cell<UmassBbbCbw>,
    /// `csw`: command status wrapper.
    pub csw: Cell<UmassBbbCsw>,
    /* CBI specific variables for transfers in progress */
    /// `cbl`: command block.
    pub cbl: Cell<UmassCbiCbl>,
    /// `sbl`: status block.
    pub sbl: Cell<UmassCbiSbl>,

    /// `transfer_xfer`: the xfer handles. Most of our operations are initiated from
    /// interrupt context, so we need to avoid using the one that is in use. We want to
    /// avoid allocating them in the interrupt context as well.
    pub transfer_xfer: [Cell<Option<&'static UsbdXfer>>; XFER_NR],

    /// `data_buffer`: the data xfer's DMA buffer (`UMASS_MAX_TRANSFER_SIZE` bytes).
    pub data_buffer: Cell<*mut u8>,

    /// `transfer_dir`: data direction.
    pub transfer_dir: Cell<i32>,
    /// `transfer_data`: data buffer.
    pub transfer_data: Cell<*mut u8>,
    /// `transfer_datalen`: (maximum) length.
    pub transfer_datalen: Cell<i32>,
    /// `transfer_actlen`: actual length.
    pub transfer_actlen: Cell<i32>,
    /// `transfer_cb`: callback.
    pub transfer_cb: Cell<Option<UmassCallback>>,
    /// `transfer_priv`: for callback.
    pub transfer_priv: Cell<*mut c_void>,
    /// `transfer_status`.
    pub transfer_status: Cell<i32>,

    /// `transfer_state`: `TSTATE_*`.
    pub transfer_state: Cell<i32>,

    /// `timeout`: in msecs.
    pub timeout: Cell<u32>,

    /// `maxlun`: max lun supported.
    pub maxlun: Cell<u8>,

    /// `sc_xfer_flags`: added to every xfer's flags (`USBD_SYNCHRONOUS` when polling).
    pub sc_xfer_flags: Cell<u16>,
    /// `sc_refcnt`.
    pub sc_refcnt: Cell<i32>,
    /// `sc_sense`: a REQUEST SENSE is in progress.
    pub sc_sense: Cell<i32>,

    /// `bus`: bus dependent data.
    pub bus: Cell<Option<NonNull<UmassScsiSoftc>>>,

    /* For polled transfers */
    /// `polling_depth`.
    pub polling_depth: Cell<i32>,
    /// `polled_xfer_status`.
    pub polled_xfer_status: Cell<UsbdStatus>,
    /// `next_polled_xfer`.
    pub next_polled_xfer: Cell<Option<&'static UsbdXfer>>,
}

impl UmassSoftc {
    /// `sc->sc_udev`, which the attach sets first.
    pub fn udev(&self) -> &'static UsbdDevice {
        match self.sc_udev.get() {
            Some(d) => d,
            None => panic(format_args!("{}: no usb device", self.sc_dev.xname())),
        }
    }

    /// `sc->sc_iface`, which the attach sets first.
    pub fn iface(&self) -> &'static UsbdInterface {
        match self.sc_iface.get() {
            Some(i) => i,
            None => panic(format_args!("{}: no usb interface", self.sc_dev.xname())),
        }
    }

    /// `sc->sc_pipe[i]`, open once the attach got that far.
    pub fn pipe(&self, i: usize) -> &'static UsbdPipe {
        match self.sc_pipe[i].get() {
            Some(p) => p,
            None => panic(format_args!("{}: pipe {i} not open", self.sc_dev.xname())),
        }
    }

    /// `sc->transfer_xfer[i]`, allocated once the attach got that far.
    pub fn xfer(&self, i: usize) -> &'static UsbdXfer {
        match self.transfer_xfer[i].get() {
            Some(x) => x,
            None => panic(format_args!("{}: no xfer {i}", self.sc_dev.xname())),
        }
    }

    /// `sc->sc_methods`, set at the end of a successful attach.
    pub fn methods(&self) -> &'static UmassWireMethods {
        match self.sc_methods.get() {
            Some(m) => m,
            None => panic(format_args!("{}: no wire methods", self.sc_dev.xname())),
        }
    }
}

// SAFETY: `#[repr(C)]` with the device first; every other member is a `Cell` of an integer,
// a raw pointer, an `Option` of a reference, a `NonNull` or a function pointer, a wire
// structure of bytes, or a `UsbdStatus` (whose 0 is `USBD_NORMAL_COMPLETION`): all valid as
// zero bits.
unsafe impl Softc for UmassSoftc {}

/// `UMASS_MAX_TRANSFER_SIZE`.
pub const UMASS_MAX_TRANSFER_SIZE: usize = MAXBSIZE;

const _: () = {
    assert!(size_of::<UmassBbbCbw>() == UMASS_BBB_CBW_SIZE as usize);
    assert!(size_of::<UmassBbbCsw>() == UMASS_BBB_CSW_SIZE as usize);
    assert!(size_of::<UmassCbiSbl>() == 2);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_header() {
        let defs = crate::reftest::defines("sys/dev/usb/umassvar.h");
        let _ = crate::reftest::assert_defines!(defs;
            UFI_COMMAND_LENGTH, DIR_NONE, DIR_IN, DIR_OUT, UMASS_BULKIN, UMASS_BULKOUT,
            UMASS_INTRIN, UMASS_NEP, UR_BBB_RESET, UR_BBB_GET_MAX_LUN, CBWSIGNATURE,
            CBWFLAGS_OUT, CBWFLAGS_IN, CBWCDBLENGTH, UMASS_BBB_CBW_SIZE, CSWSIGNATURE,
            CSWSIGNATURE_OLYMPUS_C1, CSWSTATUS_GOOD, CSWSTATUS_FAILED, CSWSTATUS_PHASE,
            UMASS_BBB_CSW_SIZE, UR_CBI_ADSC, IDB_TYPE_CCI, IDB_VALUE_PASS, IDB_VALUE_FAIL,
            IDB_VALUE_PHASE, IDB_VALUE_PERSISTENT, IDB_VALUE_STATUS_MASK, STATUS_CMD_OK,
            STATUS_CMD_UNKNOWN, STATUS_CMD_FAILED, STATUS_WIRE_FAILED, UMASS_WPROTO_UNSPEC,
            UMASS_WPROTO_BBB, UMASS_WPROTO_CBI, UMASS_WPROTO_CBI_I, UMASS_CPROTO_UNSPEC,
            UMASS_CPROTO_SCSI, UMASS_CPROTO_ATAPI, UMASS_CPROTO_UFI, UMASS_CPROTO_RBC,
            UMASS_CPROTO_ISD_ATA, UMASS_QUIRK_WRONG_CSWSIG, UMASS_QUIRK_WRONG_CSWTAG,
            UMASS_QUIRK_IGNORE_RESIDUE, XFER_BBB_CBW, XFER_BBB_DATA, XFER_BBB_DCLEAR,
            XFER_BBB_CSW1, XFER_BBB_CSW2, XFER_BBB_SCLEAR, XFER_BBB_RESET1, XFER_BBB_RESET2,
            XFER_BBB_RESET3, XFER_CBI_CB, XFER_CBI_DATA, XFER_CBI_STATUS, XFER_CBI_DCLEAR,
            XFER_CBI_SCLEAR, XFER_CBI_RESET1, XFER_CBI_RESET2, XFER_CBI_RESET3, XFER_NR,
            TSTATE_IDLE, TSTATE_BBB_COMMAND, TSTATE_BBB_DATA, TSTATE_BBB_DCLEAR,
            TSTATE_BBB_STATUS1, TSTATE_BBB_SCLEAR, TSTATE_BBB_STATUS2, TSTATE_BBB_RESET1,
            TSTATE_BBB_RESET2, TSTATE_BBB_RESET3, TSTATE_CBI_COMMAND, TSTATE_CBI_DATA,
            TSTATE_CBI_STATUS, TSTATE_CBI_DCLEAR, TSTATE_CBI_SCLEAR, TSTATE_CBI_RESET1,
            TSTATE_CBI_RESET2, TSTATE_CBI_RESET3, TSTATE_STATES,
        );
    }
}
/* </TESTS> */
