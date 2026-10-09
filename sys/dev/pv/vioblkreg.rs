/* <LICENSES> */
/*
 * Copyright (c) 2012 Stefan Fritsch.
 * Copyright (c) 2010 Minoura Makoto.
 * Copyright (c) 1998, 2001 Manuel Bouyer.
 * All rights reserved.
 *
 * This code is based in part on the NetBSD ld_virtio driver and the
 * OpenBSD wd driver.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *	notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *	notice, this list of conditions and the following disclaimer in the
 *	documentation and/or other materials provided with the distribution.
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
/* </LICENSES> */

/* <CODE> */
//! `<dev/pv/vioblkreg.h>`: the virtio block device's configuration registers, feature bits,
//! request types and status codes, and the request header.
//!
//! Upstream: sys/dev/pv/vioblkreg.h @ 3ce1f3f79392
//!
//! A request on the queue is the header (device-readable), the payload of
//! `512 * sector`-addressed bytes (absent for a flush) and one status byte the device writes.
//!
//! ## Deviations
//! - The configuration register offsets are `i32`, the type the `virtio_read_device_config_*`
//!   functions take (`dev/pv/virtiovar.rs`); the feature bits are `u64` like the core's.
//! - `struct virtio_blk_req_hdr` is `#[repr(C, packed)]` (the C's `__packed`); its member
//!   `type` is `r#type`. The fields are in the CPU's byte order, as in C: both archs are
//!   little-endian, which is what virtio 1.0 devices expect.

// Configuration registers

/// `VIRTIO_BLK_CONFIG_CAPACITY`: 64bit.
pub const VIRTIO_BLK_CONFIG_CAPACITY: i32 = 0;
/// `VIRTIO_BLK_CONFIG_SIZE_MAX`: 32bit.
pub const VIRTIO_BLK_CONFIG_SIZE_MAX: i32 = 8;
/// `VIRTIO_BLK_CONFIG_SEG_MAX`: 32bit.
pub const VIRTIO_BLK_CONFIG_SEG_MAX: i32 = 12;
/// `VIRTIO_BLK_CONFIG_GEOMETRY_C`: 16bit.
pub const VIRTIO_BLK_CONFIG_GEOMETRY_C: i32 = 16;
/// `VIRTIO_BLK_CONFIG_GEOMETRY_H`: 8bit.
pub const VIRTIO_BLK_CONFIG_GEOMETRY_H: i32 = 18;
/// `VIRTIO_BLK_CONFIG_GEOMETRY_S`: 8bit.
pub const VIRTIO_BLK_CONFIG_GEOMETRY_S: i32 = 19;
/// `VIRTIO_BLK_CONFIG_BLK_SIZE`: 32bit.
pub const VIRTIO_BLK_CONFIG_BLK_SIZE: i32 = 20;

// Feature bits

/// `VIRTIO_BLK_F_BARRIER`.
pub const VIRTIO_BLK_F_BARRIER: u64 = 1 << 0;
/// `VIRTIO_BLK_F_SIZE_MAX`.
pub const VIRTIO_BLK_F_SIZE_MAX: u64 = 1 << 1;
/// `VIRTIO_BLK_F_SEG_MAX`.
pub const VIRTIO_BLK_F_SEG_MAX: u64 = 1 << 2;
/// `VIRTIO_BLK_F_GEOMETRY`.
pub const VIRTIO_BLK_F_GEOMETRY: u64 = 1 << 4;
/// `VIRTIO_BLK_F_RO`.
pub const VIRTIO_BLK_F_RO: u64 = 1 << 5;
/// `VIRTIO_BLK_F_BLK_SIZE`.
pub const VIRTIO_BLK_F_BLK_SIZE: u64 = 1 << 6;
/// `VIRTIO_BLK_F_SCSI`.
pub const VIRTIO_BLK_F_SCSI: u64 = 1 << 7;
/// `VIRTIO_BLK_F_FLUSH`.
pub const VIRTIO_BLK_F_FLUSH: u64 = 1 << 9;
/// `VIRTIO_BLK_F_TOPOLOGY`.
pub const VIRTIO_BLK_F_TOPOLOGY: u64 = 1 << 10;
/// `VIRTIO_BLK_F_CONFIG_WCE`.
pub const VIRTIO_BLK_F_CONFIG_WCE: u64 = 1 << 11;
/// `VIRTIO_BLK_F_MQ`.
pub const VIRTIO_BLK_F_MQ: u64 = 1 << 12;
/// `VIRTIO_BLK_F_DISCARD`.
pub const VIRTIO_BLK_F_DISCARD: u64 = 1 << 13;
/// `VIRTIO_BLK_F_WRITE_ZEROES`.
pub const VIRTIO_BLK_F_WRITE_ZEROES: u64 = 1 << 14;
/// `VIRTIO_BLK_F_LIFETIME`.
pub const VIRTIO_BLK_F_LIFETIME: u64 = 1 << 15;
/// `VIRTIO_BLK_F_SECURE_ERASE`.
pub const VIRTIO_BLK_F_SECURE_ERASE: u64 = 1 << 16;

// Command

/// `VIRTIO_BLK_T_IN`: read.
pub const VIRTIO_BLK_T_IN: u32 = 0;
/// `VIRTIO_BLK_T_OUT`: write.
pub const VIRTIO_BLK_T_OUT: u32 = 1;
/// `VIRTIO_BLK_T_SCSI_CMD`.
pub const VIRTIO_BLK_T_SCSI_CMD: u32 = 2;
/// `VIRTIO_BLK_T_SCSI_CMD_OUT`.
pub const VIRTIO_BLK_T_SCSI_CMD_OUT: u32 = 3;
/// `VIRTIO_BLK_T_FLUSH`.
pub const VIRTIO_BLK_T_FLUSH: u32 = 4;
/// `VIRTIO_BLK_T_FLUSH_OUT`.
pub const VIRTIO_BLK_T_FLUSH_OUT: u32 = 5;
/// `VIRTIO_BLK_T_GET_ID`: from qemu, not in spec, yet.
pub const VIRTIO_BLK_T_GET_ID: u32 = 8;
/// `VIRTIO_BLK_T_BARRIER`.
pub const VIRTIO_BLK_T_BARRIER: u32 = 0x8000_0000;

// Status

/// `VIRTIO_BLK_S_OK`.
pub const VIRTIO_BLK_S_OK: u8 = 0;
/// `VIRTIO_BLK_S_IOERR`.
pub const VIRTIO_BLK_S_IOERR: u8 = 1;
/// `VIRTIO_BLK_S_UNSUPP`.
pub const VIRTIO_BLK_S_UNSUPP: u8 = 2;

/// `VIRTIO_BLK_ID_BYTES`: length of serial number.
pub const VIRTIO_BLK_ID_BYTES: usize = 20;

/// `VIRTIO_BLK_SECTOR_SIZE`: the unit of [`VirtioBlkReqHdr::sector`].
pub const VIRTIO_BLK_SECTOR_SIZE: u32 = 512;

/// `struct virtio_blk_req_hdr`: request header structure. A `512 * sector` byte payload and
/// a 1 byte status follow it on the queue.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtioBlkReqHdr {
    /// `type`: `VIRTIO_BLK_T_*`.
    pub r#type: u32,
    /// `ioprio`.
    pub ioprio: u32,
    /// `sector`.
    pub sector: u64,
}

const _: () = assert!(size_of::<VirtioBlkReqHdr>() == 16);
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/pv/vioblkreg.h");
        let names = crate::reftest::assert_defines!(defs;
            VIRTIO_BLK_CONFIG_CAPACITY, VIRTIO_BLK_CONFIG_SIZE_MAX, VIRTIO_BLK_CONFIG_SEG_MAX,
            VIRTIO_BLK_CONFIG_GEOMETRY_C, VIRTIO_BLK_CONFIG_GEOMETRY_H,
            VIRTIO_BLK_CONFIG_GEOMETRY_S, VIRTIO_BLK_CONFIG_BLK_SIZE,
            VIRTIO_BLK_F_BARRIER, VIRTIO_BLK_F_SIZE_MAX, VIRTIO_BLK_F_SEG_MAX,
            VIRTIO_BLK_F_GEOMETRY, VIRTIO_BLK_F_RO, VIRTIO_BLK_F_BLK_SIZE, VIRTIO_BLK_F_SCSI,
            VIRTIO_BLK_F_FLUSH, VIRTIO_BLK_F_TOPOLOGY, VIRTIO_BLK_F_CONFIG_WCE, VIRTIO_BLK_F_MQ,
            VIRTIO_BLK_F_DISCARD, VIRTIO_BLK_F_WRITE_ZEROES, VIRTIO_BLK_F_LIFETIME,
            VIRTIO_BLK_F_SECURE_ERASE,
            VIRTIO_BLK_T_IN, VIRTIO_BLK_T_OUT, VIRTIO_BLK_T_SCSI_CMD, VIRTIO_BLK_T_SCSI_CMD_OUT,
            VIRTIO_BLK_T_FLUSH, VIRTIO_BLK_T_FLUSH_OUT, VIRTIO_BLK_T_GET_ID,
            VIRTIO_BLK_T_BARRIER,
            VIRTIO_BLK_S_OK, VIRTIO_BLK_S_IOERR, VIRTIO_BLK_S_UNSUPP,
            VIRTIO_BLK_ID_BYTES, VIRTIO_BLK_SECTOR_SIZE,
        );
        crate::reftest::assert_complete(&defs, "VIRTIO_BLK_", &names);
    }
}
/* </TESTS> */
