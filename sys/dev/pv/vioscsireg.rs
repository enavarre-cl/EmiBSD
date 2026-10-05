/*	$OpenBSD: vioscsireg.h,v 1.2 2019/03/24 18:21:12 sf Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2013 Google Inc.
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

//! `<dev/pv/vioscsireg.h>`: the virtio SCSI host adapter's configuration registers, feature
//! bits, response status values, task attributes and the request and response headers.
//!
//! Upstream: sys/dev/pv/vioscsireg.h @ 3ce1f3f79392
//!
//! A request on the request queue is the request header (device-readable), the data-out
//! bytes (if any), the response header (device-writable) and the data-in bytes (if any).
//!
//! ## Deviations
//! - The configuration register offsets are `i32`, the type the `virtio_read_device_config_*`
//!   functions take (`dev/pv/virtiovar.rs`); the feature bits are `u64` like the core's.
//! - The two headers are `#[repr(C, packed)]` (the C's `__packed`); the fields are in the
//!   CPU's byte order, as in C: both archs are little-endian, which is what virtio 1.0
//!   devices expect.

// Configuration registers

/// `VIRTIO_SCSI_CONFIG_NUM_QUEUES`: 32bit.
pub const VIRTIO_SCSI_CONFIG_NUM_QUEUES: i32 = 0;
/// `VIRTIO_SCSI_CONFIG_SEG_MAX`: 32bit.
pub const VIRTIO_SCSI_CONFIG_SEG_MAX: i32 = 4;
/// `VIRTIO_SCSI_CONFIG_MAX_SECTORS`: 32bit.
pub const VIRTIO_SCSI_CONFIG_MAX_SECTORS: i32 = 8;
/// `VIRTIO_SCSI_CONFIG_CMD_PER_LUN`: 32bit.
pub const VIRTIO_SCSI_CONFIG_CMD_PER_LUN: i32 = 12;
/// `VIRTIO_SCSI_CONFIG_EVENT_INFO_SIZE`: 32bit.
pub const VIRTIO_SCSI_CONFIG_EVENT_INFO_SIZE: i32 = 16;
/// `VIRTIO_SCSI_CONFIG_SENSE_SIZE`: 32bit.
pub const VIRTIO_SCSI_CONFIG_SENSE_SIZE: i32 = 20;
/// `VIRTIO_SCSI_CONFIG_CDB_SIZE`: 32bit.
pub const VIRTIO_SCSI_CONFIG_CDB_SIZE: i32 = 24;
/// `VIRTIO_SCSI_CONFIG_MAX_CHANNEL`: 16bit.
pub const VIRTIO_SCSI_CONFIG_MAX_CHANNEL: i32 = 28;
/// `VIRTIO_SCSI_CONFIG_MAX_TARGET`: 16bit.
pub const VIRTIO_SCSI_CONFIG_MAX_TARGET: i32 = 30;
/// `VIRTIO_SCSI_CONFIG_MAX_LUN`: 32bit.
pub const VIRTIO_SCSI_CONFIG_MAX_LUN: i32 = 32;

// Feature bits

/// `VIRTIO_SCSI_F_INOUT`.
pub const VIRTIO_SCSI_F_INOUT: u64 = 1 << 0;
/// `VIRTIO_SCSI_F_HOTPLUG`.
pub const VIRTIO_SCSI_F_HOTPLUG: u64 = 1 << 1;

// Response status values

/// `VIRTIO_SCSI_S_OK`.
pub const VIRTIO_SCSI_S_OK: u8 = 0;
/// `VIRTIO_SCSI_S_OVERRUN`.
pub const VIRTIO_SCSI_S_OVERRUN: u8 = 1;
/// `VIRTIO_SCSI_S_ABORTED`.
pub const VIRTIO_SCSI_S_ABORTED: u8 = 2;
/// `VIRTIO_SCSI_S_BAD_TARGET`.
pub const VIRTIO_SCSI_S_BAD_TARGET: u8 = 3;
/// `VIRTIO_SCSI_S_RESET`.
pub const VIRTIO_SCSI_S_RESET: u8 = 4;
/// `VIRTIO_SCSI_S_BUSY`.
pub const VIRTIO_SCSI_S_BUSY: u8 = 5;
/// `VIRTIO_SCSI_S_TRANSPORT_FAILURE`.
pub const VIRTIO_SCSI_S_TRANSPORT_FAILURE: u8 = 6;
/// `VIRTIO_SCSI_S_TARGET_FAILURE`.
pub const VIRTIO_SCSI_S_TARGET_FAILURE: u8 = 7;
/// `VIRTIO_SCSI_S_NEXUS_FAILURE`.
pub const VIRTIO_SCSI_S_NEXUS_FAILURE: u8 = 8;
/// `VIRTIO_SCSI_S_FAILURE`.
pub const VIRTIO_SCSI_S_FAILURE: u8 = 9;

// Task attributes

/// `VIRTIO_SCSI_S_SIMPLE`.
pub const VIRTIO_SCSI_S_SIMPLE: u8 = 0;
/// `VIRTIO_SCSI_S_ORDERED`.
pub const VIRTIO_SCSI_S_ORDERED: u8 = 1;
/// `VIRTIO_SCSI_S_HEAD`.
pub const VIRTIO_SCSI_S_HEAD: u8 = 2;
/// `VIRTIO_SCSI_S_ACA`.
pub const VIRTIO_SCSI_S_ACA: u8 = 3;

/// `struct virtio_scsi_req_hdr`: request header structure. Followed by data-out.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtioScsiReqHdr {
    /// `lun`.
    pub lun: [u8; 8],
    /// `id`.
    pub id: u64,
    /// `task_attr`: `VIRTIO_SCSI_S_SIMPLE` ...
    pub task_attr: u8,
    /// `prio`.
    pub prio: u8,
    /// `crn`.
    pub crn: u8,
    /// `cdb`.
    pub cdb: [u8; 32],
}

/// `struct virtio_scsi_res_hdr`: response header structure. Followed by data-in.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtioScsiResHdr {
    /// `sense_len`.
    pub sense_len: u32,
    /// `residual`.
    pub residual: u32,
    /// `status_qualifier`.
    pub status_qualifier: u16,
    /// `status`.
    pub status: u8,
    /// `response`: `VIRTIO_SCSI_S_OK` ...
    pub response: u8,
    /// `sense`.
    pub sense: [u8; 96],
}

impl VirtioScsiResHdr {
    /// All zeros, as the request slots start.
    pub const fn new() -> Self {
        Self {
            sense_len: 0,
            residual: 0,
            status_qualifier: 0,
            status: 0,
            response: 0,
            sense: [0; 96],
        }
    }
}

impl Default for VirtioScsiResHdr {
    fn default() -> Self {
        Self::new()
    }
}

const _: () = {
    assert!(size_of::<VirtioScsiReqHdr>() == 51);
    assert!(size_of::<VirtioScsiResHdr>() == 108);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/pv/vioscsireg.h");
        let names = crate::reftest::assert_defines!(defs;
            VIRTIO_SCSI_CONFIG_NUM_QUEUES, VIRTIO_SCSI_CONFIG_SEG_MAX,
            VIRTIO_SCSI_CONFIG_MAX_SECTORS, VIRTIO_SCSI_CONFIG_CMD_PER_LUN,
            VIRTIO_SCSI_CONFIG_EVENT_INFO_SIZE, VIRTIO_SCSI_CONFIG_SENSE_SIZE,
            VIRTIO_SCSI_CONFIG_CDB_SIZE, VIRTIO_SCSI_CONFIG_MAX_CHANNEL,
            VIRTIO_SCSI_CONFIG_MAX_TARGET, VIRTIO_SCSI_CONFIG_MAX_LUN,
            VIRTIO_SCSI_F_INOUT, VIRTIO_SCSI_F_HOTPLUG,
            VIRTIO_SCSI_S_OK, VIRTIO_SCSI_S_OVERRUN, VIRTIO_SCSI_S_ABORTED,
            VIRTIO_SCSI_S_BAD_TARGET, VIRTIO_SCSI_S_RESET, VIRTIO_SCSI_S_BUSY,
            VIRTIO_SCSI_S_TRANSPORT_FAILURE, VIRTIO_SCSI_S_TARGET_FAILURE,
            VIRTIO_SCSI_S_NEXUS_FAILURE, VIRTIO_SCSI_S_FAILURE,
            VIRTIO_SCSI_S_SIMPLE, VIRTIO_SCSI_S_ORDERED, VIRTIO_SCSI_S_HEAD,
            VIRTIO_SCSI_S_ACA,
        );
        crate::reftest::assert_complete(&defs, "VIRTIO_SCSI_", &names);
    }
}
