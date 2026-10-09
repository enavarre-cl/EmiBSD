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
/* </LICENSES> */

/* <CODE> */
//! `<sys/scsiio.h>`: the ioctls of the SCSI midlayer: raw commands (`SCIOCCOMMAND`), the
//! debug level (`SCIOCDEBUG`), a device's bus address (`SCIOCIDENTIFY`), and the bus probe
//! and detach requests of bio(4) (`SBIOCPROBE`, `SBIOCDETACH`).
//!
//! Upstream: sys/sys/scsiio.h @ 3ce1f3f79392
//!
//! The original file carries no licence text, only its `$OpenBSD$` and `$NetBSD$` lines
//! (kept above); the user accepted it for kernel ports on 2026-10-04.
//!
//! ## Deviations
//! - The structures are `#[repr(C)]` ABI types with LP64 sizes and offsets checked at
//!   compile time. `scsireq_t`'s hole after `cmdlen` is the named `_pad0`, so that every
//!   byte is a field and the structure can be read and written as plain bytes
//!   ([`AbiPod`]).
//! - `caddr_t databuf` is a `usize`: a user address, only handed to `copyin`/`copyout`
//!   (`docs/C_TO_RUST.md`). `sd_cookie` stays a raw pointer, as `bio_cookie` in `biovar.rs`.
//! - `u_long` is `u64`, `u_char` is `u8`; the `SCCMD_*` request flags are `u64` (they fill
//!   `flags`), its return statuses `u8` (they fill `retsts`), and the `TYPE_*` values `i32`
//!   (they fill `scsi_addr.type`, a raw identifier here).

use core::ffi::c_void;
use core::mem::{offset_of, size_of};

use crate::machine::copy::AbiPod;
use crate::sys::ioccom::{_io, _ior, _iow, _iowr};

/// `SENSEBUFLEN`: bytes of sense data a [`Scsireq`] returns.
pub const SENSEBUFLEN: usize = 48;
/// `CMDBUFLEN`: bytes of command a [`Scsireq`] carries.
pub const CMDBUFLEN: usize = 16;

/// `scsireq_t` (`struct scsireq`): a raw SCSI command from user space (`SCIOCCOMMAND`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scsireq {
    /// `flags`: info about the request status and type (`SCCMD_*`).
    pub flags: u64,
    /// `timeout`: in milliseconds.
    pub timeout: u64,
    /// `cmd`: the command descriptor block.
    pub cmd: [u8; CMDBUFLEN],
    /// `cmdlen`: bytes of `cmd` used.
    pub cmdlen: u8,
    /// The C compiler's padding before `databuf`.
    pub _pad0: [u8; 7],
    /// `databuf`: address in user space of the buffer.
    pub databuf: usize,
    /// `datalen`: size of the user buffer (request).
    pub datalen: u64,
    /// `datalen_used`: size of the user buffer (used).
    pub datalen_used: u64,
    /// `sense`: the returned sense data.
    pub sense: [u8; SENSEBUFLEN],
    /// `senselen`: sense data request size (at most `SENSEBUFLEN`).
    pub senselen: u8,
    /// `senselen_used`: return value only.
    pub senselen_used: u8,
    /// `status`: what the SCSI status was from the adapter.
    pub status: u8,
    /// `retsts`: the return status for the command (`SCCMD_OK` ...).
    pub retsts: u8,
    /// `error`: error bits.
    pub error: i32,
}

impl Scsireq {
    /// All zeros.
    pub const fn new() -> Self {
        Self {
            flags: 0,
            timeout: 0,
            cmd: [0; CMDBUFLEN],
            cmdlen: 0,
            _pad0: [0; 7],
            databuf: 0,
            datalen: 0,
            datalen_used: 0,
            sense: [0; SENSEBUFLEN],
            senselen: 0,
            senselen_used: 0,
            status: 0,
            retsts: 0,
            error: 0,
        }
    }
}

impl Default for Scsireq {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: `#[repr(C)]`, integers and byte arrays only, the one hole filled by `_pad0`
// (checked below): every byte is initialised and every pattern is valid.
unsafe impl AbiPod for Scsireq {}

// bit definitions for flags

/// `SCCMD_READ`: data comes in from the device.
pub const SCCMD_READ: u64 = 0x0000_0001;
/// `SCCMD_WRITE`: data goes out to the device.
pub const SCCMD_WRITE: u64 = 0x0000_0002;
/// `SCCMD_IOV`: `databuf` is an iovec.
pub const SCCMD_IOV: u64 = 0x0000_0004;
/// `SCCMD_ESCAPE`: an adapter escape.
pub const SCCMD_ESCAPE: u64 = 0x0000_0010;
/// `SCCMD_TARGET`: target mode.
pub const SCCMD_TARGET: u64 = 0x0000_0020;

// definitions for the return status (retsts)

/// `SCCMD_OK`.
pub const SCCMD_OK: u8 = 0x00;
/// `SCCMD_TIMEOUT`.
pub const SCCMD_TIMEOUT: u8 = 0x01;
/// `SCCMD_BUSY`.
pub const SCCMD_BUSY: u8 = 0x02;
/// `SCCMD_SENSE`: sense data returned.
pub const SCCMD_SENSE: u8 = 0x03;
/// `SCCMD_UNKNOWN`.
pub const SCCMD_UNKNOWN: u8 = 0x04;

/// `SCIOCCOMMAND`: run a raw SCSI command.
pub const SCIOCCOMMAND: u64 = _iowr::<Scsireq>(b'Q', 1);

/// `SC_DB_CMDS`: show all SCSI commands and errors.
pub const SC_DB_CMDS: i32 = 0x0000_0001;
/// `SC_DB_FLOW`: show routines entered.
pub const SC_DB_FLOW: i32 = 0x0000_0002;
/// `SC_DB_FLOW2`: show the path inside routines.
pub const SC_DB_FLOW2: i32 = 0x0000_0004;
/// `SC_DB_DMA`: show DMA segments etc.
pub const SC_DB_DMA: i32 = 0x0000_0008;
/// `SCIOCDEBUG`: set the debug level, from 0 to 15.
pub const SCIOCDEBUG: u64 = _iow::<i32>(b'Q', 2);

/// `struct scsi_addr`: where a device is (`SCIOCIDENTIFY`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScsiAddr {
    /// `type`: `TYPE_SCSI` or `TYPE_ATAPI`.
    pub r#type: i32,
    /// `scbus`: the scsibus unit; -1 if wildcard.
    pub scbus: i32,
    /// `target`: -1 if wildcard.
    pub target: i32,
    /// `lun`: -1 if wildcard.
    pub lun: i32,
}

// SAFETY: `#[repr(C)]`, four `int`s, no padding: any pattern is valid.
unsafe impl AbiPod for ScsiAddr {}

/// `TYPE_SCSI`: a real SCSI target.
pub const TYPE_SCSI: i32 = 0;
/// `TYPE_ATAPI`: an emulated one (ATAPI, USB mass storage).
pub const TYPE_ATAPI: i32 = 1;

/// `SCIOCRESET`: reset the device.
pub const SCIOCRESET: u64 = _io(b'Q', 7);
/// `SCIOCIDENTIFY`: where is the device.
pub const SCIOCIDENTIFY: u64 = _ior::<ScsiAddr>(b'Q', 9);

/// `struct sbioc_device`: a target and LUN of the bus a bio(4) cookie names.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SbiocDevice {
    /// `sd_cookie`: the bio(4) cookie of the bus.
    pub sd_cookie: *mut c_void,
    /// `sd_target`.
    pub sd_target: i32,
    /// `sd_lun`.
    pub sd_lun: i32,
}

impl Default for SbiocDevice {
    fn default() -> Self {
        Self {
            sd_cookie: core::ptr::null_mut(),
            sd_target: 0,
            sd_lun: 0,
        }
    }
}

// SAFETY: `#[repr(C)]`, a pointer and two `int`s, no padding (checked below); a raw pointer
// is an address, valid with any bits, and is never dereferenced through this structure.
unsafe impl AbiPod for SbiocDevice {}

/// `SBIOCPROBE`: probe a target or LUN of a bus.
pub const SBIOCPROBE: u64 = _iowr::<SbiocDevice>(b'Q', 127);
/// `SBIOCDETACH`: detach a target or LUN of a bus.
pub const SBIOCDETACH: u64 = _iowr::<SbiocDevice>(b'Q', 128);

const _: () = {
    assert!(size_of::<Scsireq>() == 120);
    assert!(offset_of!(Scsireq, cmdlen) == 32);
    assert!(offset_of!(Scsireq, databuf) == 40);
    assert!(offset_of!(Scsireq, sense) == 64);
    assert!(offset_of!(Scsireq, retsts) == 115);
    assert!(offset_of!(Scsireq, error) == 116);
    assert!(size_of::<ScsiAddr>() == 16);
    assert!(size_of::<SbiocDevice>() == 16);
    assert!(offset_of!(SbiocDevice, sd_lun) == 12);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_have_openbsd_values() {
        // The values the C compiler computes on LP64 for _IOWR('Q', 1, scsireq_t) and the
        // others.
        assert_eq!(SCIOCCOMMAND, 0xc078_5101);
        assert_eq!(SCIOCDEBUG, 0x8004_5102);
        assert_eq!(SCIOCRESET, 0x2000_5107);
        assert_eq!(SCIOCIDENTIFY, 0x4010_5109);
        assert_eq!(SBIOCPROBE, 0xc010_517f);
        assert_eq!(SBIOCDETACH, 0xc010_5180);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/sys/scsiio.h");
        let ours = crate::reftest::assert_defines!(defs;
            SENSEBUFLEN, CMDBUFLEN, SCCMD_READ, SCCMD_WRITE, SCCMD_IOV, SCCMD_ESCAPE,
            SCCMD_TARGET, SCCMD_OK, SCCMD_TIMEOUT, SCCMD_BUSY, SCCMD_SENSE, SCCMD_UNKNOWN,
            SC_DB_CMDS, SC_DB_FLOW, SC_DB_FLOW2, SC_DB_DMA, TYPE_SCSI, TYPE_ATAPI,
        );
        crate::reftest::assert_complete(&defs, "SCCMD_", &ours);
        crate::reftest::assert_complete(&defs, "SC_DB_", &ours);
    }
}
/* </TESTS> */
