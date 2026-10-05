/*	$OpenBSD: atascsi.c,v 1.156 2024/09/04 07:54:52 mglocker Exp $ */
/*	$OpenBSD: atascsi.h,v 1.54 2022/04/09 20:10:26 naddy Exp $ */
/* <LICENSES> */
/*
 * Copyright (c) 2007 David Gwynne <dlg@openbsd.org>
 * Copyright (c) 2010 Conformal Systems LLC <info@conformal.com>
 * Copyright (c) 2010 Jonathan Matthew <jonathan@d14n.org>
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

//! `atascsi`: the SCSI to ATA translation layer of the SATA host controller drivers
//! (`ahci(4)`, ...). It attaches a scsibus for the controller (`scsibus* at scsi?`) whose
//! targets are the controller's ports and whose LUNs are the ports of a port multiplier, and
//! turns the SCSI commands of `sd(4)` and `cd(4)` into ATA commands: READ/WRITE into DMA
//! (LBA28, LBA48 or NCQ) commands, SYNCHRONIZE CACHE into FLUSH CACHE, START STOP UNIT into
//! FLUSH CACHE and STANDBY IMMEDIATE, WRITE SAME(16) and UNMAP into DATA SET MANAGEMENT (TRIM),
//! ATA PASS-THROUGH(12/16) into the command it carries; INQUIRY (with its VPD pages), READ
//! CAPACITY(10/16) and REQUEST SENSE are answered from the IDENTIFY data. ATAPI devices get
//! their SCSI command through the PACKET command; a port multiplier answers INQUIRY itself.
//!
//! Upstream: sys/dev/ata/atascsi.c @ 3ce1f3f79392, sys/dev/ata/atascsi.h @ 3ce1f3f79392
//!
//! `atascsi.h` holds the ATA commands, the IDENTIFY data, the FIS layouts, `struct ata_xfer`
//! and the interface a controller driver offers (`struct atascsi_methods`); both files share
//! this module, as `cd.c` and `cd.h` share `cd.rs`.
//!
//! ## Deviations
//! - `ata_xfer`'s `fis` and `packetcmd` point into the controller's command table, DMA memory
//!   the controller reads: they stay raw pointers, and the FIS is read and written whole and
//!   volatile through [`AtaXfer::with_fis`] (no reference into DMA memory); the packet command
//!   is copied in with volatile byte writes. `state` (`volatile int`) is an `AtomicI32` read
//!   and written `Relaxed`.
//! - `atascsi_private` (`void *`, a `struct scsi_xfer *` or a `struct scsi_iopool *`) is the
//!   enum [`AtaPrivate`]; `complete` is a `Cell` of an `Option` (calling a missing one
//!   panics where the C would jump to NULL).
//! - `struct atascsi`, `struct atascsi_host_port` and `struct atascsi_port` are `malloc(9)`ed
//!   as in C and written whole; the arrays of port pointers are `mallocarray`ed `Cell`s of
//!   `Option<NonNull<_>>` (zeroed is `None`, as the C's NULL). `ap_identify` is an
//!   `UnsafeCell` written once by `atascsi_probe` before the port is published.
//!   `atascsi_lookup_port` and `atascsi_free` also return when a host port is missing (the
//!   C would dereference NULL); the other functions find their port through it and do nothing
//!   more when it is not there.
//! - `dma_alloc(9)` (`kern/dma_alloc.c`) is not ported: the IDENTIFY buffer is a
//!   `scsiconf.rs` `DmaBuf`, the TRIM range list of UNMAP `malloc(9)`ed (`M_DEVBUF`).
//! - The functions that return an errno (`atascsi_probe`, `atascsi_detach`,
//!   `atascsi_port_identify`, `atascsi_port_set_features`, `ata_polled`) return
//!   `Result<(), Errno>`; `atascsi_passthru_map`'s 0/1 is a `Result<(), ()>`-like `bool`
//!   (`true`: mapped).
//! - Three paths of the C go on after completing the transfer, and complete it a second
//!   time: `atascsi_disk_unmap` with `SCSI_POLL` or a short CDB, and with a descriptor list
//!   shorter than one descriptor ("no work, no error"). Here they return after
//!   `atascsi_done`. The failure path of `atascsi_disk_unmap_task` frees the range list it
//!   allocated (the C frees `xa->data`, which is not yet that list).
//! - The tag cleanup loop of `atascsi_probe` stops when `scsi_io_get(SCSI_NOSLEEP)` finds no
//!   xfer (the C would dereference NULL); `atascsi_disk_sense` and `atascsi_pmp_sense` write
//!   the sense fields only within `datalen`.
//! - `ata_swapcopy` swaps the bytes of each 16-bit word over byte slices (`swap16` of the
//!   loaded word in C), so its source need not be aligned.

use core::cell::{Cell, UnsafeCell};
use core::cmp::min;
use core::ffi::c_void;
use core::mem::{offset_of, size_of};
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicI32, Ordering};

use crate::conf::vers::OSRELEASE;
use crate::dev::ata::pmreg::{SATA_PMP_CONTROL_PORT, SATA_PMP_MAX_PORTS};
use crate::kern::kern_malloc::{free, malloc, mallocarray};
use crate::kern::kern_task::{SYSTQ, task_add, task_set};
use crate::kern::subr_autoconf::{config_detach, config_found};
use crate::kern::subr_prf::{panic, printf};
use crate::machine::cpu::delay;
use crate::scsi::scsi_all::{
    ATA_PASSTHRU_12, ATA_PASSTHRU_16, ATA_PASSTHRU_PROTO_MASK, ATA_PASSTHRU_PROTO_NON_DATA,
    ATA_PASSTHRU_PROTO_PIO_DATAIN, ATA_PASSTHRU_PROTO_PIO_DATAOUT, INQUIRY, PREVENT_ALLOW,
    READ_CAP_16_TPE, READ_CAP_16_TPRZ, REQUEST_SENSE, SI_EVPD, SI_PG_ATA, SI_PG_DEVID,
    SI_PG_SERIAL, SI_PG_SUPPORTED, SID_CmdQue, SID_SCSI2_ALEN, SID_SCSI2_RESPONSE,
    SKEY_ILLEGAL_REQUEST, SKEY_NO_SENSE, SSD_EOM, SSD_ERRCODE_CURRENT, SSD_ILI, START_STOP,
    ScsiAtaPassthru12, ScsiAtaPassthru16, ScsiInquiry, ScsiInquiryData, ScsiReadCapData,
    ScsiReadCapData16, ScsiSenseData, ScsiVpdAta, ScsiVpdDevidHdr, ScsiVpdHdr, ScsiVpdSerial,
    ScsiWire, T_DIRECT, TEST_UNIT_READY, VPD_ATA_COMMAND_CODE_ATA, VPD_ATA_COMMAND_CODE_ATAPI,
    VPD_DEVID_ASSOC_LU, VPD_DEVID_CODE_ASCII, VPD_DEVID_CODE_BINARY, VPD_DEVID_TYPE_NAA,
    VPD_DEVID_TYPE_T10, wire_ref,
};
use crate::scsi::scsi_base::{
    scsi_cmd_rw_decode, scsi_copy_internal_data, scsi_done, scsi_io_get, scsi_io_put,
    scsi_iopool_init,
};
use crate::scsi::scsi_disk::{
    READ_10, READ_12, READ_16, READ_CAPACITY, READ_CAPACITY_16, READ_COMMAND, SI_PG_DISK_INFO,
    SI_PG_DISK_LIMITS, SI_PG_DISK_LIMITS_LEN_THIN, SI_PG_DISK_THIN, SSS_STOP, SYNCHRONIZE_CACHE,
    ScsiReadCapacity, ScsiReadCapacity16, ScsiStartStop, ScsiSynchronizeCache, ScsiUnmap,
    ScsiUnmapData, ScsiUnmapDesc, ScsiVpdDiskInfo, ScsiVpdDiskLimits, ScsiVpdDiskThin,
    ScsiWriteSame16, UNMAP, VPD_DISK_THIN_TPU, VPD_DISK_THIN_TPWS, WRITE_10, WRITE_12, WRITE_16,
    WRITE_COMMAND, WRITE_SAME_16, WRITE_SAME_F_UNMAP,
};
use crate::scsi::scsiconf::{
    _2btol, _4btol, _8btol, _lto2b, _lto4b, _lto8b, DmaBuf, SCSI_DATA_IN, SCSI_DATA_OUT,
    SCSI_NOSLEEP, SCSI_POLL, SCSI_REV_SPC3, SDEV_ATAPI, SDEV_NO_ADAPTER_TARGET, ScsiAdapter,
    ScsiIo, ScsiIopool, ScsiLink, ScsiXfer, ScsibusAttachArgs, ScsibusSoftc, XS_DRIVER_STUFFUP,
    XS_NOERROR, XS_SENSE, XS_TIMEOUT, scsiprint,
};
use crate::sys::buf::Buf;
use crate::sys::device::Device;
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_DEVBUF, M_WAITOK, M_ZERO};
use crate::sys::task::Task;
use crate::sys::timeout::Timeout;

/*
 * ATA commands
 */

/// `ATA_C_READDMA_EXT`.
pub const ATA_C_READDMA_EXT: u8 = 0x25;
/// `ATA_C_READ_LOG_EXT`.
pub const ATA_C_READ_LOG_EXT: u8 = 0x2f;
/// `ATA_C_WRITEDMA_EXT`.
pub const ATA_C_WRITEDMA_EXT: u8 = 0x35;
/// `ATA_C_READ_FPDMA`.
pub const ATA_C_READ_FPDMA: u8 = 0x60;
/// `ATA_C_WRITE_FPDMA`.
pub const ATA_C_WRITE_FPDMA: u8 = 0x61;
/// `ATA_C_PACKET`.
pub const ATA_C_PACKET: u8 = 0xa0;
/// `ATA_C_IDENTIFY_PACKET`.
pub const ATA_C_IDENTIFY_PACKET: u8 = 0xa1;
/// `ATA_C_READDMA`.
pub const ATA_C_READDMA: u8 = 0xc8;
/// `ATA_C_WRITEDMA`.
pub const ATA_C_WRITEDMA: u8 = 0xca;
/// `ATA_C_STANDBY_IMMED`.
pub const ATA_C_STANDBY_IMMED: u8 = 0xe0;
/// `ATA_C_READ_PM`.
pub const ATA_C_READ_PM: u8 = 0xe4;
/// `ATA_C_WRITE_PM`.
pub const ATA_C_WRITE_PM: u8 = 0xe8;
/// `ATA_C_FLUSH_CACHE`.
pub const ATA_C_FLUSH_CACHE: u8 = 0xe7;
/// `ATA_C_FLUSH_CACHE_EXT`: lba48.
pub const ATA_C_FLUSH_CACHE_EXT: u8 = 0xea;
/// `ATA_C_IDENTIFY`.
pub const ATA_C_IDENTIFY: u8 = 0xec;
/// `ATA_C_SET_FEATURES`.
pub const ATA_C_SET_FEATURES: u8 = 0xef;
/// `ATA_C_SEC_FREEZE_LOCK`.
pub const ATA_C_SEC_FREEZE_LOCK: u8 = 0xf5;
/// `ATA_C_DSM`.
pub const ATA_C_DSM: u8 = 0x06;

/*
 * ATA SET FEATURES subcommands (feature field)
 */

/// `ATA_SF_WRITECACHE_EN`.
pub const ATA_SF_WRITECACHE_EN: u8 = 0x02;
/// `ATA_SF_XFERMODE`.
pub const ATA_SF_XFERMODE: u8 = 0x03;
/// `ATA_SF_SATA_FEATURE_EN`.
pub const ATA_SF_SATA_FEATURE_EN: u8 = 0x10;
/// `ATA_SF_XFERMODE_UDMA`.
pub const ATA_SF_XFERMODE_UDMA: u8 = 0x40;
/// `ATA_SF_SATA_FEATURE_DIS`.
pub const ATA_SF_SATA_FEATURE_DIS: u8 = 0x90;
/// `ATA_SF_LOOKAHEAD_EN`.
pub const ATA_SF_LOOKAHEAD_EN: u8 = 0xaa;

/*
 * ATA SET FEATURES args (count field)
 */

/// `ATA_SF_SATA_DEVIPS`: Device-initiated power management.
pub const ATA_SF_SATA_DEVIPS: u8 = 0x03;
/// `ATA_SF_SATA_DEVAPS`: Device Automatic Partial to Slumber transitions.
pub const ATA_SF_SATA_DEVAPS: u8 = 0x07;
/// `ATA_SF_SATA_DEVSLEEP`: DevSleep power management state.
pub const ATA_SF_SATA_DEVSLEEP: u8 = 0x09;

/// `ATA_ID_VALIDINFO_ULTRADMA`.
pub const ATA_ID_VALIDINFO_ULTRADMA: u16 = 0x0004;
/// `ATA_ID_ADD_SUPPORT_DRT`.
pub const ATA_ID_ADD_SUPPORT_DRT: u16 = 0x4000;

/// `ATA_QDEPTH(_q)`.
pub const fn ata_qdepth(q: u16) -> i32 {
    (q & 0x1f) as i32 + 1
}

/// `ATA_SATACAP_GEN1`.
pub const ATA_SATACAP_GEN1: u16 = 0x0002;
/// `ATA_SATACAP_GEN2`.
pub const ATA_SATACAP_GEN2: u16 = 0x0004;
/// `ATA_SATACAP_GEN3`.
pub const ATA_SATACAP_GEN3: u16 = 0x0008;
/// `ATA_SATACAP_NCQ`.
pub const ATA_SATACAP_NCQ: u16 = 0x0100;
/// `ATA_SATACAP_HIPM`.
pub const ATA_SATACAP_HIPM: u16 = 0x0200;
/// `ATA_SATACAP_HOSTAPS`.
pub const ATA_SATACAP_HOSTAPS: u16 = 0x2000;
/// `ATA_SATACAP_DEVAPS`.
pub const ATA_SATACAP_DEVAPS: u16 = 0x4000;
/// `ATA_SATAFSUP_DIPM`.
pub const ATA_SATAFSUP_DIPM: u16 = 0x0008;
/// `ATA_SATAFSUP_DEVSLP`.
pub const ATA_SATAFSUP_DEVSLP: u16 = 0x0100;
/// `ATA_SATAFEN_DIPM`.
pub const ATA_SATAFEN_DIPM: u16 = 0x0008;
/// `ATA_SATAFEN_DEVSLP`.
pub const ATA_SATAFEN_DEVSLP: u16 = 0x0100;
/// `ATA_ID_F87_WWN`.
pub const ATA_ID_F87_WWN: u16 = 1 << 8;
/// `ATA_ID_P2L_SECT_MASK`.
pub const ATA_ID_P2L_SECT_MASK: u16 = 0xc000;
/// `ATA_ID_P2L_SECT_VALID`.
pub const ATA_ID_P2L_SECT_VALID: u16 = 0x4000;
/// `ATA_ID_P2L_SECT_SET`.
pub const ATA_ID_P2L_SECT_SET: u16 = 0x2000;
/// `ATA_ID_P2L_SECT_SIZESET`.
pub const ATA_ID_P2L_SECT_SIZESET: u16 = 0x1000;
/// `ATA_ID_P2L_SECT_SIZE`.
pub const ATA_ID_P2L_SECT_SIZE: u16 = 0x000f;
/// `ATA_ID_FORM_MASK`.
pub const ATA_ID_FORM_MASK: u16 = 0x000f;
/// `ATA_ID_DATA_SET_MGMT_TRIM`.
pub const ATA_ID_DATA_SET_MGMT_TRIM: u16 = 0x0001;
/// `ATA_ID_LALIGN_MASK`.
pub const ATA_ID_LALIGN_MASK: u16 = 0xc000;
/// `ATA_ID_LALIGN_VALID`.
pub const ATA_ID_LALIGN_VALID: u16 = 0x4000;
/// `ATA_ID_LALIGN`.
pub const ATA_ID_LALIGN: u16 = 0x3fff;

/*
 * IDENTIFY DEVICE data
 */

/// `ATA_IDENTIFY_WRITECACHE`.
pub const ATA_IDENTIFY_WRITECACHE: u16 = 1 << 5;
/// `ATA_IDENTIFY_LOOKAHEAD`.
pub const ATA_IDENTIFY_LOOKAHEAD: u16 = 1 << 6;

/*
 * ATA DSM (Data Set Management) subcommands
 */

/// `ATA_DSM_TRIM`.
pub const ATA_DSM_TRIM: u8 = 0x01;

/// `ATA_DSM_TRIM_DESC(_lba, _len)`: one TRIM range entry.
pub const fn ata_dsm_trim_desc(lba: u64, len: u32) -> u64 {
    lba | ((len as u64) << 48)
}

/// `ATA_DSM_TRIM_MAX_LEN`.
pub const ATA_DSM_TRIM_MAX_LEN: u32 = 0xffff;

/*
 * Frame Information Structures
 */

/// `ATA_FIS_LENGTH`.
pub const ATA_FIS_LENGTH: usize = 20;

/// `ATA_FIS_TYPE_H2D`.
pub const ATA_FIS_TYPE_H2D: u8 = 0x27;
/// `ATA_H2D_FLAGS_CMD`.
pub const ATA_H2D_FLAGS_CMD: u8 = 1 << 7;
/// `ATA_H2D_FEATURES_DMA`.
pub const ATA_H2D_FEATURES_DMA: u8 = 1 << 0;
/// `ATA_H2D_FEATURES_DIR`.
pub const ATA_H2D_FEATURES_DIR: u8 = 1 << 2;
/// `ATA_H2D_FEATURES_DIR_READ`.
pub const ATA_H2D_FEATURES_DIR_READ: u8 = 1 << 2;
/// `ATA_H2D_FEATURES_DIR_WRITE`.
pub const ATA_H2D_FEATURES_DIR_WRITE: u8 = 0 << 2;
/// `ATA_H2D_DEVICE_LBA`.
pub const ATA_H2D_DEVICE_LBA: u8 = 0x40;
/// `ATA_FIS_CONTROL_SRST`.
pub const ATA_FIS_CONTROL_SRST: u8 = 0x04;
/// `ATA_FIS_CONTROL_4BIT`.
pub const ATA_FIS_CONTROL_4BIT: u8 = 0x08;

/// `ATA_FIS_TYPE_D2H`.
pub const ATA_FIS_TYPE_D2H: u8 = 0x34;
/// `ATA_D2H_FLAGS_INTR`.
pub const ATA_D2H_FLAGS_INTR: u8 = 1 << 6;

/// `ATA_LOG_10H_TYPE_NOTQUEUED`.
pub const ATA_LOG_10H_TYPE_NOTQUEUED: u8 = 0x80;
/// `ATA_LOG_10H_TYPE_TAG_MASK`.
pub const ATA_LOG_10H_TYPE_TAG_MASK: u8 = 0x1f;

/*
 * SATA registers
 */

/// `SATA_SStatus_DET`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_DET: u32 = 0x00f;
/// `SATA_SStatus_DET_NODEV`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_DET_NODEV: u32 = 0x000;
/// `SATA_SStatus_DET_NOPHY`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_DET_NOPHY: u32 = 0x001;
/// `SATA_SStatus_DET_DEV`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_DET_DEV: u32 = 0x003;
/// `SATA_SStatus_DET_OFFLINE`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_DET_OFFLINE: u32 = 0x004;
/// `SATA_SStatus_SPD`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_SPD: u32 = 0x0f0;
/// `SATA_SStatus_SPD_NONE`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_SPD_NONE: u32 = 0x000;
/// `SATA_SStatus_SPD_1_5`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_SPD_1_5: u32 = 0x010;
/// `SATA_SStatus_SPD_3_0`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_SPD_3_0: u32 = 0x020;
/// `SATA_SStatus_SPD_6_0`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_SPD_6_0: u32 = 0x030;
/// `SATA_SStatus_IPM`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_IPM: u32 = 0xf00;
/// `SATA_SStatus_IPM_NODEV`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_IPM_NODEV: u32 = 0x000;
/// `SATA_SStatus_IPM_ACTIVE`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_IPM_ACTIVE: u32 = 0x100;
/// `SATA_SStatus_IPM_PARTIAL`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_IPM_PARTIAL: u32 = 0x200;
/// `SATA_SStatus_IPM_SLUMBER`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_IPM_SLUMBER: u32 = 0x600;
/// `SATA_SStatus_IPM_DEVSLEEP`.
#[allow(non_upper_case_globals)] // the C name
pub const SATA_SStatus_IPM_DEVSLEEP: u32 = 0x800;

/// `SATA_SIGNATURE_PORT_MULTIPLIER`.
pub const SATA_SIGNATURE_PORT_MULTIPLIER: u32 = 0x96690101;
/// `SATA_SIGNATURE_ATAPI`.
pub const SATA_SIGNATURE_ATAPI: u32 = 0xeb140101;
/// `SATA_SIGNATURE_DISK`.
pub const SATA_SIGNATURE_DISK: u32 = 0x00000101;

/// `ATA_F_READ`.
pub const ATA_F_READ: i32 = 1 << 0;
/// `ATA_F_WRITE`.
pub const ATA_F_WRITE: i32 = 1 << 1;
/// `ATA_F_NOWAIT`.
pub const ATA_F_NOWAIT: i32 = 1 << 2;
/// `ATA_F_POLL`.
pub const ATA_F_POLL: i32 = 1 << 3;
/// `ATA_F_PIO`.
pub const ATA_F_PIO: i32 = 1 << 4;
/// `ATA_F_PACKET`.
pub const ATA_F_PACKET: i32 = 1 << 5;
/// `ATA_F_NCQ`.
pub const ATA_F_NCQ: i32 = 1 << 6;
/// `ATA_F_DONE`.
pub const ATA_F_DONE: i32 = 1 << 7;
/// `ATA_F_GET_RFIS`.
pub const ATA_F_GET_RFIS: i32 = 1 << 8;
/// `ATA_FMT_FLAGS`: the `%b` format of `ATA_F_*`.
pub const ATA_FMT_FLAGS: &[u8] =
    b"\x10\x09GET_RFIS\x08DONE\x07NCQ\x06PACKET\x05PIO\x04POLL\x03NOWAIT\x02WRITE\x01READ";

/// `ATA_S_SETUP`.
pub const ATA_S_SETUP: i32 = 0;
/// `ATA_S_PENDING`.
pub const ATA_S_PENDING: i32 = 1;
/// `ATA_S_COMPLETE`.
pub const ATA_S_COMPLETE: i32 = 2;
/// `ATA_S_ERROR`.
pub const ATA_S_ERROR: i32 = 3;
/// `ATA_S_TIMEOUT`.
pub const ATA_S_TIMEOUT: i32 = 4;
/// `ATA_S_ONCHIP`.
pub const ATA_S_ONCHIP: i32 = 5;
/// `ATA_S_PUT`.
pub const ATA_S_PUT: i32 = 6;
/// `ATA_S_DONE`.
pub const ATA_S_DONE: i32 = 7;

/// `ASAA_CAP_NCQ`.
pub const ASAA_CAP_NCQ: i32 = 1 << 0;
/// `ASAA_CAP_NEEDS_RESERVED`.
pub const ASAA_CAP_NEEDS_RESERVED: i32 = 1 << 1;
/// `ASAA_CAP_PMP_NCQ`.
pub const ASAA_CAP_PMP_NCQ: i32 = 1 << 2;

/// `ATA_PORT_T_NONE`.
pub const ATA_PORT_T_NONE: i32 = 0;
/// `ATA_PORT_T_DISK`.
pub const ATA_PORT_T_DISK: i32 = 1;
/// `ATA_PORT_T_ATAPI`.
pub const ATA_PORT_T_ATAPI: i32 = 2;
/// `ATA_PORT_T_PM`.
pub const ATA_PORT_T_PM: i32 = 3;

/// `ATA_PORT_F_NCQ` (`atascsi.c`).
pub const ATA_PORT_F_NCQ: i32 = 0x1;
/// `ATA_PORT_F_TRIM` (`atascsi.c`).
pub const ATA_PORT_F_TRIM: i32 = 0x2;

/// `struct ata_identify`: the IDENTIFY (PACKET) DEVICE data, 256 little-endian words.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AtaIdentify {
    /// `config`: 0.
    pub config: u16,
    /// `ncyls`: 1.
    pub ncyls: u16,
    /// `reserved1`: 2.
    pub reserved1: u16,
    /// `nheads`: 3.
    pub nheads: u16,
    /// `track_size`: 4.
    pub track_size: u16,
    /// `sector_size`: 5.
    pub sector_size: u16,
    /// `nsectors`: 6.
    pub nsectors: u16,
    /// `reserved2`: 7, vendor unique.
    pub reserved2: [u16; 3],
    /// `serial`: 10.
    pub serial: [u8; 20],
    /// `buffer_type`: 20.
    pub buffer_type: u16,
    /// `buffer_size`: 21.
    pub buffer_size: u16,
    /// `ecc`: 22.
    pub ecc: u16,
    /// `firmware`: 23.
    pub firmware: [u8; 8],
    /// `model`: 27.
    pub model: [u8; 40],
    /// `multi`: 47.
    pub multi: u16,
    /// `dwcap`: 48.
    pub dwcap: u16,
    /// `cap`: 49.
    pub cap: u16,
    /// `reserved3`: 50.
    pub reserved3: u16,
    /// `piomode`: 51.
    pub piomode: u16,
    /// `dmamode`: 52.
    pub dmamode: u16,
    /// `validinfo`: 53.
    pub validinfo: u16,
    /// `curcyls`: 54.
    pub curcyls: u16,
    /// `curheads`: 55.
    pub curheads: u16,
    /// `cursectrk`: 56.
    pub cursectrk: u16,
    /// `curseccp`: 57.
    pub curseccp: [u16; 2],
    /// `mult2`: 59.
    pub mult2: u16,
    /// `addrsec`: 60.
    pub addrsec: [u16; 2],
    /// `worddma`: 62.
    pub worddma: u16,
    /// `dworddma`: 63.
    pub dworddma: u16,
    /// `advpiomode`: 64.
    pub advpiomode: u16,
    /// `minmwdma`: 65.
    pub minmwdma: u16,
    /// `recmwdma`: 66.
    pub recmwdma: u16,
    /// `minpio`: 67.
    pub minpio: u16,
    /// `minpioflow`: 68.
    pub minpioflow: u16,
    /// `add_support`: 69.
    pub add_support: u16,
    /// `reserved4`: 70.
    pub reserved4: u16,
    /// `typtime`: 71.
    pub typtime: [u16; 2],
    /// `reserved5`: 73.
    pub reserved5: [u16; 2],
    /// `qdepth`: 75.
    pub qdepth: u16,
    /// `satacap`: 76.
    pub satacap: u16,
    /// `reserved6`: 77.
    pub reserved6: u16,
    /// `satafsup`: 78.
    pub satafsup: u16,
    /// `satafen`: 79.
    pub satafen: u16,
    /// `majver`: 80.
    pub majver: u16,
    /// `minver`: 81.
    pub minver: u16,
    /// `cmdset82`: 82.
    pub cmdset82: u16,
    /// `cmdset83`: 83.
    pub cmdset83: u16,
    /// `cmdset84`: 84.
    pub cmdset84: u16,
    /// `features85`: 85.
    pub features85: u16,
    /// `features86`: 86.
    pub features86: u16,
    /// `features87`: 87.
    pub features87: u16,
    /// `ultradma`: 88.
    pub ultradma: u16,
    /// `erasetime`: 89.
    pub erasetime: u16,
    /// `erasetimex`: 90.
    pub erasetimex: u16,
    /// `apm`: 91.
    pub apm: u16,
    /// `masterpw`: 92.
    pub masterpw: u16,
    /// `hwreset`: 93.
    pub hwreset: u16,
    /// `acoustic`: 94.
    pub acoustic: u16,
    /// `stream_min`: 95.
    pub stream_min: u16,
    /// `stream_xfer_d`: 96.
    pub stream_xfer_d: u16,
    /// `stream_lat`: 97.
    pub stream_lat: u16,
    /// `streamperf`: 98.
    pub streamperf: [u16; 2],
    /// `addrsecxt`: 100.
    pub addrsecxt: [u16; 4],
    /// `stream_xfer_p`: 104.
    pub stream_xfer_p: u16,
    /// `padding1`: 105.
    pub padding1: u16,
    /// `p2l_sect`: 106.
    pub p2l_sect: u16,
    /// `seek_delay`: 107.
    pub seek_delay: u16,
    /// `naa_ieee_oui`: 108.
    pub naa_ieee_oui: u16,
    /// `ieee_oui_uid`: 109.
    pub ieee_oui_uid: u16,
    /// `uid_mid`: 110.
    pub uid_mid: u16,
    /// `uid_low`: 111.
    pub uid_low: u16,
    /// `resv_wwn`: 112.
    pub resv_wwn: [u16; 4],
    /// `incits`: 116.
    pub incits: u16,
    /// `words_lsec`: 117.
    pub words_lsec: [u16; 2],
    /// `cmdset119`: 119.
    pub cmdset119: u16,
    /// `features120`: 120.
    pub features120: u16,
    /// `padding2`.
    pub padding2: [u16; 6],
    /// `rmsn`: 127.
    pub rmsn: u16,
    /// `securestatus`: 128.
    pub securestatus: u16,
    /// `vendor`: 129.
    pub vendor: [u16; 31],
    /// `padding3`: 160.
    pub padding3: [u16; 8],
    /// `form`: 168.
    pub form: u16,
    /// `data_set_mgmt`: 169.
    pub data_set_mgmt: u16,
    /// `padding4`: 170.
    pub padding4: [u16; 6],
    /// `curmedser`: 176.
    pub curmedser: [u16; 30],
    /// `sctsupport`: 206.
    pub sctsupport: u16,
    /// `rpm`: 207.
    pub rpm: u16,
    /// `padding5`: 208.
    pub padding5: [u16; 1],
    /// `logical_align`: 209.
    pub logical_align: u16,
    /// `padding6`: 210.
    pub padding6: [u16; 45],
    /// `integrity`: 255.
    pub integrity: u16,
}

impl AtaIdentify {
    /// The data as the 512 bytes the device sent.
    pub fn as_bytes(&self) -> &[u8; 512] {
        // SAFETY: `#[repr(C)]` of `u16`s and even-length byte arrays at even offsets: no
        // padding (the size is asserted to be 512 below), so every byte is initialised; the
        // array (alignment 1) borrows `self`.
        unsafe { &*ptr::from_ref(self).cast::<[u8; 512]>() }
    }

    /// The 16-bit word at word offset `off` (`letoh16` of a member).
    fn word(&self, off: usize) -> u16 {
        let b = self.as_bytes();
        u16::from_le_bytes([b[2 * off], b[2 * off + 1]])
    }
}

/// `struct ata_fis_h2d`: a Register Host to Device FIS.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AtaFisH2d {
    /// `type`: `ATA_FIS_TYPE_H2D`.
    pub r#type: u8,
    /// `flags`: `ATA_H2D_FLAGS_CMD` and the port multiplier port.
    pub flags: u8,
    /// `command`.
    pub command: u8,
    /// `features`.
    pub features: u8,

    /// `lba_low`.
    pub lba_low: u8,
    /// `lba_mid`.
    pub lba_mid: u8,
    /// `lba_high`.
    pub lba_high: u8,
    /// `device`.
    pub device: u8,

    /// `lba_low_exp`.
    pub lba_low_exp: u8,
    /// `lba_mid_exp`.
    pub lba_mid_exp: u8,
    /// `lba_high_exp`.
    pub lba_high_exp: u8,
    /// `features_exp`.
    pub features_exp: u8,

    /// `sector_count`.
    pub sector_count: u8,
    /// `sector_count_exp`.
    pub sector_count_exp: u8,
    /// `reserved0`.
    pub reserved0: u8,
    /// `control`.
    pub control: u8,

    /// `reserved1`.
    pub reserved1: u8,
    /// `reserved2`.
    pub reserved2: u8,
    /// `reserved3`.
    pub reserved3: u8,
    /// `reserved4`.
    pub reserved4: u8,
}

/// `struct ata_fis_d2h`: a Register Device to Host FIS.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AtaFisD2h {
    /// `type`: `ATA_FIS_TYPE_D2H`.
    pub r#type: u8,
    /// `flags`.
    pub flags: u8,
    /// `status`.
    pub status: u8,
    /// `error`.
    pub error: u8,

    /// `lba_low`.
    pub lba_low: u8,
    /// `lba_mid`.
    pub lba_mid: u8,
    /// `lba_high`.
    pub lba_high: u8,
    /// `device`.
    pub device: u8,

    /// `lba_low_exp`.
    pub lba_low_exp: u8,
    /// `lba_mid_exp`.
    pub lba_mid_exp: u8,
    /// `lba_high_exp`.
    pub lba_high_exp: u8,
    /// `reserved0`.
    pub reserved0: u8,

    /// `sector_count`.
    pub sector_count: u8,
    /// `sector_count_exp`.
    pub sector_count_exp: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `reserved2`.
    pub reserved2: u8,

    /// `reserved3`.
    pub reserved3: u8,
    /// `reserved4`.
    pub reserved4: u8,
    /// `reserved5`.
    pub reserved5: u8,
    /// `reserved6`.
    pub reserved6: u8,
}

/// `struct ata_log_page_10h`: SATA log page 10h, which looks like a D2H FIS with the errored
/// tag number in the first byte.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AtaLogPage10h {
    /// `err_regs`.
    pub err_regs: AtaFisD2h,
    /// `reserved`.
    pub reserved: [u8; 256 - size_of::<AtaFisD2h>()],
    /// `vendor_specific`.
    pub vendor_specific: [u8; 255],
    /// `checksum`.
    pub checksum: u8,
}

// SAFETY: `#[repr(C)]` of `u8`s and byte arrays only (sizes asserted below): alignment 1,
// no padding, every bit pattern valid.
unsafe impl ScsiWire for AtaFisH2d {}
// SAFETY: as above.
unsafe impl ScsiWire for AtaFisD2h {}
// SAFETY: as above.
unsafe impl ScsiWire for AtaLogPage10h {}

/// What `atascsi_private` points at.
#[derive(Clone, Copy)]
pub enum AtaPrivate {
    /// Nothing yet.
    None,
    /// The SCSI transfer the command carries out.
    Xs(&'static ScsiXfer),
    /// The host port's iopool a polled internal command goes back to (`ata_polled`).
    Iopool(&'static ScsiIopool),
}

/// `struct ata_xfer`: one ATA command, as the controller driver hands it out (it is the first
/// member of the driver's command control block).
pub struct AtaXfer {
    /// `fis`: the command FIS, in the controller's command table (DMA memory).
    pub fis: Cell<*mut AtaFisH2d>,
    /// `rfis`: the D2H FIS the device answered with, when asked for (`ATA_F_GET_RFIS`).
    pub rfis: Cell<AtaFisD2h>,
    /// `packetcmd`: the ATAPI command, in the command table (16 bytes).
    pub packetcmd: Cell<*mut u8>,
    /// `tag`: the command slot.
    pub tag: Cell<u8>,

    /// `data`.
    pub data: Cell<*mut u8>,
    /// `datalen`.
    pub datalen: Cell<usize>,
    /// `resid`.
    pub resid: Cell<usize>,

    /// `complete`.
    pub complete: Cell<Option<fn(xa: &'static AtaXfer)>>,
    /// `task`: for work deferred to a task queue (UNMAP).
    pub task: Task,
    /// `stimeout`: the controller driver's command timeout.
    pub stimeout: Timeout,
    /// `timeout`: in milliseconds.
    pub timeout: Cell<u32>,

    /// `flags`: `ATA_F_*`.
    pub flags: Cell<i32>,

    /// `state`: `ATA_S_*` (`volatile int`).
    pub state: AtomicI32,

    /// `atascsi_private`.
    pub atascsi_private: Cell<AtaPrivate>,

    /// `pmp_port`.
    pub pmp_port: Cell<i32>,
}

impl AtaXfer {
    /// A transfer whose FIS and packet command live at `fis` and `packetcmd`, for slot
    /// `tag`, in state `ATA_S_SETUP`.
    pub const fn new(fis: *mut AtaFisH2d, packetcmd: *mut u8, tag: u8) -> Self {
        Self {
            fis: Cell::new(fis),
            rfis: Cell::new(AtaFisD2h {
                r#type: 0,
                flags: 0,
                status: 0,
                error: 0,
                lba_low: 0,
                lba_mid: 0,
                lba_high: 0,
                device: 0,
                lba_low_exp: 0,
                lba_mid_exp: 0,
                lba_high_exp: 0,
                reserved0: 0,
                sector_count: 0,
                sector_count_exp: 0,
                reserved1: 0,
                reserved2: 0,
                reserved3: 0,
                reserved4: 0,
                reserved5: 0,
                reserved6: 0,
            }),
            packetcmd: Cell::new(packetcmd),
            tag: Cell::new(tag),
            data: Cell::new(ptr::null_mut()),
            datalen: Cell::new(0),
            resid: Cell::new(0),
            complete: Cell::new(None),
            task: Task::zeroed(),
            stimeout: Timeout::zeroed(),
            timeout: Cell::new(0),
            flags: Cell::new(0),
            state: AtomicI32::new(ATA_S_SETUP),
            atascsi_private: Cell::new(AtaPrivate::None),
            pmp_port: Cell::new(0),
        }
    }

    /// `xa->state`.
    #[inline]
    pub fn state(&self) -> i32 {
        self.state.load(Ordering::Relaxed)
    }

    /// `xa->state = state`.
    #[inline]
    pub fn set_state(&self, state: i32) {
        self.state.store(state, Ordering::Relaxed)
    }

    /// `xa->fis->...`: reads the command FIS, lets `f` change it and writes it back. The FIS is
    /// DMA memory the controller reads once the command is started; the driver owns it until
    /// then, so the read-modify-write is the C's member stores.
    pub fn with_fis<R>(&self, f: impl FnOnce(&mut AtaFisH2d) -> R) -> R {
        let p = self.fis.get();
        if p.is_null() {
            panic(format_args!("ata_xfer {:p} without a FIS", self));
        }
        // SAFETY: the controller driver points `fis` at the command FIS of this transfer's
        // command table, which lives as long as the transfer and which the controller does
        // not read before the command is started; `AtaFisH2d` has alignment 1.
        let mut fis = unsafe { ptr::read_volatile(p) };
        let r = f(&mut fis);
        // SAFETY: as above.
        unsafe { ptr::write_volatile(p, fis) };
        r
    }

    /// `memcpy(xa->packetcmd, cmd, len)`: the ATAPI command, at most 16 bytes.
    pub fn set_packetcmd(&self, cmd: &[u8]) {
        let p = self.packetcmd.get();
        if p.is_null() {
            panic(format_args!("ata_xfer {:p} without a packet command", self));
        }
        for (i, &b) in cmd.iter().take(16).enumerate() {
            // SAFETY: `packetcmd` is the 16-byte ATAPI command area of this transfer's
            // command table (DMA memory, hence volatile); `i` is below 16.
            unsafe { ptr::write_volatile(p.add(i), b) };
        }
    }

    /// `xa->complete(xa)`.
    fn run_complete(&'static self) {
        match self.complete.get() {
            Some(f) => f(self),
            None => panic(format_args!("ata_xfer {:p} without a completion", self)),
        }
    }

    /// `xa->atascsi_private` as the transfer it carries out.
    fn xs(&self) -> &'static ScsiXfer {
        match self.atascsi_private.get() {
            AtaPrivate::Xs(xs) => xs,
            _ => panic(format_args!("ata_xfer {:p} without a scsi_xfer", self)),
        }
    }
}

/// `ata_probe` of [`AtascsiMethods`].
///
/// # Safety
///
/// `cookie` is the `aaa_cookie` the controller driver gave `atascsi_attach`.
pub type AtaProbeFn = unsafe fn(cookie: *mut c_void, port: i32, lun: i32) -> i32;
/// `ata_free` of [`AtascsiMethods`].
///
/// # Safety
///
/// As for [`AtaProbeFn`].
pub type AtaFreeFn = unsafe fn(cookie: *mut c_void, port: i32, lun: i32);
/// `ata_get_xfer` of [`AtascsiMethods`].
///
/// # Safety
///
/// As for [`AtaProbeFn`]; `port` is a port the controller probed.
pub type AtaGetXferFn = unsafe fn(cookie: *mut c_void, port: i32) -> Option<&'static AtaXfer>;

/// `struct atascsi_methods`: what a controller driver offers atascsi.
pub struct AtascsiMethods {
    /// `ata_probe`: the `ATA_PORT_T_*` of a port (`lun` 0) or of a port multiplier's port.
    pub ata_probe: AtaProbeFn,
    /// `ata_free`.
    pub ata_free: AtaFreeFn,
    /// `ata_get_xfer`: a free command of a port.
    pub ata_get_xfer: AtaGetXferFn,
    /// `ata_put_xfer`.
    pub ata_put_xfer: fn(xa: &'static AtaXfer),
    /// `ata_cmd`: issues a command; its `complete` runs when it is over (before returning for
    /// `ATA_F_POLL`).
    pub ata_cmd: fn(xa: &'static AtaXfer),
}

/// `struct atascsi_attach_args`.
pub struct AtascsiAttachArgs {
    /// `aaa_cookie`.
    pub aaa_cookie: *mut c_void,

    /// `aaa_methods`.
    pub aaa_methods: &'static AtascsiMethods,
    /// `aaa_minphys`.
    pub aaa_minphys: Option<fn(bp: &Buf, link: &'static ScsiLink)>,
    /// `aaa_nports`.
    pub aaa_nports: i32,
    /// `aaa_ncmds`.
    pub aaa_ncmds: i32,
    /// `aaa_capability`: `ASAA_CAP_*`.
    pub aaa_capability: i32,
}

/// `struct atascsi`.
pub struct Atascsi {
    /// `as_dev`: the controller.
    pub as_dev: &'static Device,
    /// `as_cookie`: the controller's `aaa_cookie`.
    pub as_cookie: *mut c_void,

    /// `as_host_ports`: `as_nports` entries, indexed by target.
    pub as_host_ports: *const Cell<Option<NonNull<AtascsiHostPort>>>,
    /// The number of `as_host_ports` (`aaa_nports`; the C keeps it as the bus width).
    pub as_nports: usize,

    /// `as_methods`.
    pub as_methods: &'static AtascsiMethods,
    /// `as_switch`: `atascsi_switch` with the controller's `dev_minphys`.
    pub as_switch: ScsiAdapter,
    /// `as_scsibus`.
    pub as_scsibus: Cell<Option<&'static ScsibusSoftc>>,

    /// `as_capability`.
    pub as_capability: i32,
    /// `as_ncqdepth`.
    pub as_ncqdepth: i32,
}

impl Atascsi {
    /// `&as->as_host_ports[port]`.
    fn host_port_slot(&self, port: usize) -> Option<&Cell<Option<NonNull<AtascsiHostPort>>>> {
        if port >= self.as_nports {
            return None;
        }
        // SAFETY: `atascsi_attach` allocated (zeroed) `as_nports` entries, freed only by
        // `atascsi_detach`.
        Some(unsafe { &*self.as_host_ports.add(port) })
    }

    /// `as->as_host_ports[port]`.
    pub fn host_port(&self, port: usize) -> Option<&'static AtascsiHostPort> {
        let ahp = self.host_port_slot(port)?.get()?;
        // SAFETY: `atascsi_probe` stores a host port it allocated, freed by `atascsi_free`
        // after it clears the entry.
        Some(unsafe { ahp.as_ref() })
    }
}

/// `struct atascsi_host_port`: a port of the host controller.
pub struct AtascsiHostPort {
    /// `ahp_iopool`: the port's commands.
    pub ahp_iopool: ScsiIopool,
    /// `ahp_as`.
    pub ahp_as: &'static Atascsi,
    /// `ahp_port`.
    pub ahp_port: i32,
    /// `ahp_nports`: 1, or `SATA_PMP_MAX_PORTS` with a port multiplier.
    pub ahp_nports: i32,

    /// `ahp_ports`: `ahp_nports` entries, indexed by LUN.
    pub ahp_ports: *const Cell<Option<NonNull<AtascsiPort>>>,
}

impl AtascsiHostPort {
    /// `&ahp->ahp_ports[lun]`.
    fn port_slot(&self, lun: usize) -> Option<&Cell<Option<NonNull<AtascsiPort>>>> {
        if lun >= self.ahp_nports.max(0) as usize {
            return None;
        }
        // SAFETY: `atascsi_probe` allocated (zeroed) `ahp_nports` entries, which live as
        // long as the host port.
        Some(unsafe { &*self.ahp_ports.add(lun) })
    }

    /// `ahp->ahp_ports[lun]`.
    pub fn port(&self, lun: usize) -> Option<&'static AtascsiPort> {
        let ap = self.port_slot(lun)?.get()?;
        // SAFETY: `atascsi_probe` stores a port it allocated, freed by `atascsi_free` after
        // it clears the entry.
        Some(unsafe { ap.as_ref() })
    }
}

/// `struct atascsi_port`: any port (of the controller or of a port multiplier) and the
/// device on it.
pub struct AtascsiPort {
    /// `ap_identify`: written once by `atascsi_probe`, before the port is published.
    pub ap_identify: UnsafeCell<AtaIdentify>,
    /// `ap_host_port`.
    pub ap_host_port: &'static AtascsiHostPort,
    /// `ap_as`.
    pub ap_as: &'static Atascsi,
    /// `ap_pmp_port`.
    pub ap_pmp_port: i32,
    /// `ap_type`.
    pub ap_type: i32,
    /// `ap_ncqdepth`.
    pub ap_ncqdepth: Cell<i32>,
    /// `ap_features`: `ATA_PORT_F_*`.
    pub ap_features: Cell<i32>,
}

impl AtascsiPort {
    /// `ap->ap_identify`.
    pub fn identify(&self) -> &AtaIdentify {
        // SAFETY: written only by `atascsi_probe` before the port is reachable through
        // `ahp_ports`, never while a reference from here is alive.
        unsafe { &*self.ap_identify.get() }
    }

    /// `ap->ap_pmp_port` as the FIS's port multiplier field.
    fn pmp_flags(&self) -> u8 {
        ATA_H2D_FLAGS_CMD | self.ap_pmp_port as u8
    }
}

/// `atascsi_switch`: the template of the adapter's entry points.
pub const ATASCSI_SWITCH: ScsiAdapter = ScsiAdapter {
    scsi_cmd: atascsi_cmd,
    dev_minphys: None,
    dev_probe: Some(atascsi_probe),
    dev_free: Some(atascsi_free),
    ioctl: None,
};

/// `link->bus->sb_adapter_softc`: the atascsi a link is on.
fn atascsi_link_as(link: &ScsiLink) -> &'static Atascsi {
    let p = link.bus().sb_adapter_softc.get();
    if p.is_null() {
        panic(format_args!("atascsi: bus without an adapter softc"));
    }
    // SAFETY: `atascsi_attach` attaches its scsibus with its `struct atascsi` as
    // `saa_adapter_softc`, and only that bus's links reach `as_switch`; it is freed by
    // `atascsi_detach` after the bus is detached.
    unsafe { &*p.cast::<Atascsi>().cast_const() }
}

/// `xs->io`: the ata_xfer the midlayer took for a transfer.
fn atascsi_xs_xa(xs: &ScsiXfer) -> &'static AtaXfer {
    let Some(io) = xs.io.get() else {
        panic(format_args!("atascsi: xs {:p} without an xfer", xs));
    };
    // SAFETY: the link's pool is its host port's `ahp_iopool`, whose `io_get`
    // (`atascsi_io_get`) hands out the controller's `ata_xfer`s, which live as long as the
    // controller's port.
    unsafe { io.cast::<AtaXfer>().as_ref() }
}

/// `atascsi_attach`: attaches the scsibus of a SATA controller.
pub fn atascsi_attach(self_: &'static Device, aaa: &AtascsiAttachArgs) -> &'static Atascsi {
    let mut switch = ATASCSI_SWITCH;
    // copy from template and modify for ourselves
    if aaa.aaa_minphys.is_some() {
        switch.dev_minphys = aaa.aaa_minphys;
    }

    let nports = aaa.aaa_nports.max(0) as usize;
    let Some(host_ports) = mallocarray(
        nports,
        size_of::<Cell<Option<NonNull<AtascsiHostPort>>>>(),
        M_DEVBUF,
        M_WAITOK | M_ZERO,
    ) else {
        panic(format_args!("atascsi_attach: mallocarray(M_WAITOK) failed"));
    };

    let Some(mem) = malloc(size_of::<Atascsi>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("atascsi_attach: malloc(M_WAITOK) failed"));
    };
    let asp = mem.cast::<Atascsi>();
    // SAFETY: a fresh allocation of `size_of::<Atascsi>()` bytes (malloc aligns it), written
    // whole before any use; it lives until atascsi_detach.
    let as_: &'static Atascsi = unsafe {
        asp.as_ptr().write(Atascsi {
            as_dev: self_,
            as_cookie: aaa.aaa_cookie,
            // Zeroed: every entry is `None`.
            as_host_ports: host_ports.as_ptr().cast(),
            as_nports: nports,
            as_methods: aaa.aaa_methods,
            as_switch: switch,
            as_scsibus: Cell::new(None),
            as_capability: aaa.aaa_capability,
            as_ncqdepth: aaa.aaa_ncmds,
        });
        &*asp.as_ptr()
    };

    let mut saa = ScsibusAttachArgs::new();
    saa.saa_adapter = Some(&as_.as_switch);
    saa.saa_adapter_softc = asp.as_ptr().cast();
    saa.saa_adapter_buswidth = aaa.aaa_nports as u16;
    saa.saa_luns = SATA_PMP_MAX_PORTS as u8;
    saa.saa_adapter_target = SDEV_NO_ADAPTER_TARGET;
    saa.saa_openings = 1;
    saa.saa_pool = None;
    saa.saa_quirks = 0;
    saa.saa_flags = 0;
    saa.saa_wwpn = 0;
    saa.saa_wwnn = 0;

    let bus = config_found(self_, ptr::from_mut(&mut saa).cast(), Some(scsiprint));
    // SAFETY: what attaches at the controller is a scsibus (`scsibus* at scsi?`), whose softc
    // begins with its device; softcs are never freed while attached.
    as_.as_scsibus
        .set(bus.map(|d| unsafe { &*d.as_ptr().cast::<ScsibusSoftc>().cast_const() }));

    as_
}

/// `atascsi_detach`.
///
/// # Safety
///
/// `as_` came from [`atascsi_attach`] and nothing uses it after a successful return.
pub unsafe fn atascsi_detach(as_: &'static Atascsi, flags: i32) -> Result<(), Errno> {
    if let Some(bus) = as_.as_scsibus.get() {
        // SAFETY: the scsibus atascsi_attach attached, which the caller gives up.
        unsafe { config_detach(NonNull::from(&bus.sc_dev), flags) }?;
    }

    let nports = as_.as_nports;
    if let Some(hp) = NonNull::new(as_.as_host_ports.cast_mut()) {
        free(
            hp.cast(),
            M_DEVBUF,
            nports * size_of::<Cell<Option<NonNull<AtascsiHostPort>>>>(),
        );
    }
    free(NonNull::from(as_).cast(), M_DEVBUF, size_of::<Atascsi>());

    Ok(())
}

/// `atascsi_lookup_port`: the port behind a link.
pub fn atascsi_lookup_port(link: &ScsiLink) -> Option<&'static AtascsiPort> {
    let as_ = atascsi_link_as(link);

    if link.target.get() >= link.bus().sb_adapter_buswidth.get() {
        return None;
    }

    let ahp = as_.host_port(link.target.get() as usize)?;
    if i32::from(link.lun.get()) >= ahp.ahp_nports {
        return None;
    }

    ahp.port(link.lun.get() as usize)
}

/// `atascsi_probe`: the adapter's `dev_probe`: asks the controller what is on the port,
/// identifies the device and sets its transfer mode, NCQ depth, write cache and look-ahead,
/// and freezes its security state.
pub fn atascsi_probe(link: &'static ScsiLink) -> Result<(), Errno> {
    let as_ = atascsi_link_as(link);

    let port = i32::from(link.target.get());
    if link.target.get() >= link.bus().sb_adapter_buswidth.get() {
        return Err(Errno::ENXIO);
    }
    let lun = i32::from(link.lun.get());

    // if this is a PMP port, check it's valid
    if lun > 0 {
        match as_.host_port(port as usize) {
            Some(ahp) if lun < ahp.ahp_nports => {}
            _ => return Err(Errno::ENXIO),
        }
    }

    // SAFETY: `as_cookie` is the controller's `aaa_cookie`.
    let type_ = unsafe { (as_.as_methods.ata_probe)(as_.as_cookie, port, lun) };
    let unsupported = |rv: Errno| {
        // SAFETY: as above.
        unsafe { (as_.as_methods.ata_free)(as_.as_cookie, port, lun) };
        Err(rv)
    };
    match type_ {
        ATA_PORT_T_DISK => {}
        ATA_PORT_T_ATAPI => link.flags.set(link.flags.get() | SDEV_ATAPI),
        ATA_PORT_T_PM => {
            if lun != 0 {
                printf(format_args!(
                    "{}.{}.{}: Port multipliers cannot be nested\n",
                    as_.as_dev.xname(),
                    port,
                    lun
                ));
                return unsupported(Errno::ENODEV);
            }
        }
        _ => return unsupported(Errno::ENODEV),
    }

    let ahp: &'static AtascsiHostPort = if lun == 0 {
        let nports = if type_ == ATA_PORT_T_PM {
            SATA_PMP_MAX_PORTS
        } else {
            1
        };
        let Some(ports) = mallocarray(
            nports as usize,
            size_of::<Cell<Option<NonNull<AtascsiPort>>>>(),
            M_DEVBUF,
            M_WAITOK | M_ZERO,
        ) else {
            panic(format_args!("atascsi_probe: mallocarray(M_WAITOK) failed"));
        };
        let Some(mem) = malloc(size_of::<AtascsiHostPort>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
            panic(format_args!("atascsi_probe: malloc(M_WAITOK) failed"));
        };
        let ahpp = mem.cast::<AtascsiHostPort>();
        // SAFETY: a fresh allocation of `size_of::<AtascsiHostPort>()` bytes, written whole
        // before any use; it lives until atascsi_free.
        let ahp: &'static AtascsiHostPort = unsafe {
            ahpp.as_ptr().write(AtascsiHostPort {
                ahp_iopool: ScsiIopool::new(),
                ahp_as: as_,
                ahp_port: port,
                ahp_nports: nports,
                // Zeroed: every entry is `None`.
                ahp_ports: ports.as_ptr().cast(),
            });
            &*ahpp.as_ptr()
        };

        // SAFETY: atascsi_io_get and atascsi_io_put take this host port as their cookie,
        // which lives as long as the pool.
        unsafe {
            scsi_iopool_init(
                &ahp.ahp_iopool,
                ahpp.as_ptr().cast(),
                atascsi_io_get,
                atascsi_io_put,
            )
        };

        if let Some(slot) = as_.host_port_slot(port as usize) {
            slot.set(Some(ahpp));
        }
        ahp
    } else {
        match as_.host_port(port as usize) {
            Some(ahp) => ahp,
            None => return unsupported(Errno::ENXIO),
        }
    };

    let ap_pmp_port = if lun == 0 {
        if type_ == ATA_PORT_T_PM {
            SATA_PMP_CONTROL_PORT
        } else {
            0
        }
    } else {
        lun - 1
    };

    let Some(mem) = malloc(size_of::<AtascsiPort>(), M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!("atascsi_probe: malloc(M_WAITOK) failed"));
    };
    let app = mem.cast::<AtascsiPort>();
    // SAFETY: a fresh allocation of `size_of::<AtascsiPort>()` bytes; the identify data is
    // zeroed in place (the allocation is) rather than built on the stack, and the rest is
    // written member by member before any use; it lives until atascsi_free.
    let ap: &'static AtascsiPort = unsafe {
        let p = app.as_ptr();
        ptr::addr_of_mut!((*p).ap_host_port).write(ahp);
        ptr::addr_of_mut!((*p).ap_as).write(as_);
        ptr::addr_of_mut!((*p).ap_pmp_port).write(ap_pmp_port);
        ptr::addr_of_mut!((*p).ap_type).write(type_);
        ptr::addr_of_mut!((*p).ap_ncqdepth).write(Cell::new(0));
        ptr::addr_of_mut!((*p).ap_features).write(Cell::new(0));
        &*p
    };

    link.pool.set(Some(&ahp.ahp_iopool));

    // fetch the device info, except for port multipliers
    if type_ != ATA_PORT_T_PM {
        // devices attached to port multipliers tend not to be spun up at this point, and
        // sometimes this prevents identification from working, so we retry a few times with
        // a fairly long delay.

        let Some(mut identify) = DmaBuf::new(size_of::<AtaIdentify>(), M_WAITOK) else {
            panic(format_args!("atascsi_probe: dma_alloc(PR_WAITOK) failed"));
        };

        let mut count = if lun > 0 { 6 } else { 2 };
        let mut rv = Err(Errno::EIO);
        while count > 0 {
            count -= 1;
            rv = atascsi_port_identify(ap, identify.bytes());
            if rv.is_ok() {
                let src = identify.bytes();
                // SAFETY: `ap_identify` is the port's own, not yet published, and 512 bytes
                // (`AtaIdentify` accepts any bytes: integers and byte arrays).
                unsafe {
                    ptr::copy_nonoverlapping(
                        src.as_ptr(),
                        ap.ap_identify.get().cast::<u8>(),
                        size_of::<AtaIdentify>(),
                    )
                };
                break;
            }
            if count > 0 {
                delay(5_000_000);
            }
        }

        drop(identify);

        if let Err(rv) = rv {
            // error:
            free(app.cast(), M_DEVBUF, size_of::<AtascsiPort>());
            return unsupported(rv);
        }
    }

    if let Some(slot) = ahp.port_slot(lun as usize) {
        slot.set(Some(app));
    }

    if type_ != ATA_PORT_T_DISK {
        return Ok(());
    }

    // Early SATA drives (as well as PATA drives) need to have their transfer mode set
    // properly, otherwise commands that use DMA will time out.
    let id = ap.identify();
    let mut xfermode = None;
    let validinfo = id.word(offset_of!(AtaIdentify, validinfo) / 2);
    if validinfo & ATA_ID_VALIDINFO_ULTRADMA != 0 {
        let ultradma = id.word(offset_of!(AtaIdentify, ultradma) / 2);
        xfermode = (0..=7u8)
            .rev()
            .find(|i| ultradma & (1 << i) != 0)
            .map(|i| ATA_SF_XFERMODE_UDMA | i);
    }
    if let Some(xfermode) = xfermode {
        let _ = atascsi_port_set_features(ap, ATA_SF_XFERMODE, xfermode);
    }

    if as_.as_capability & ASAA_CAP_NCQ != 0
        && id.word(offset_of!(AtaIdentify, satacap) / 2) & ATA_SATACAP_NCQ != 0
        && (lun == 0 || as_.as_capability & ASAA_CAP_PMP_NCQ != 0)
    {
        ap.ap_ncqdepth
            .set(ata_qdepth(id.word(offset_of!(AtaIdentify, qdepth) / 2)));
        let mut qdepth = min(ap.ap_ncqdepth.get(), as_.as_ncqdepth);
        if as_.as_capability & ASAA_CAP_NEEDS_RESERVED != 0 {
            qdepth -= 1;
        }

        if qdepth > 1 {
            ap.ap_features.set(ap.ap_features.get() | ATA_PORT_F_NCQ);

            // Raise the number of openings
            link.openings.set(qdepth as u16);

            // XXX for directly attached devices, throw away any xfers that have tag numbers
            // higher than what the device supports.
            if lun == 0 {
                while qdepth > 0 {
                    qdepth -= 1;
                    let Some(io) = scsi_io_get(&ahp.ahp_iopool, SCSI_NOSLEEP) else {
                        break;
                    };
                    // SAFETY: an opening of the host port's pool: a controller ata_xfer.
                    let xa = unsafe { io.cast::<AtaXfer>().as_ref() };
                    if u16::from(xa.tag.get()) < link.openings.get() {
                        xa.set_state(ATA_S_COMPLETE);
                        scsi_io_put(&ahp.ahp_iopool, io);
                    }
                }
            }
        }
    }

    if id.word(offset_of!(AtaIdentify, data_set_mgmt) / 2) & ATA_ID_DATA_SET_MGMT_TRIM != 0 {
        ap.ap_features.set(ap.ap_features.get() | ATA_PORT_F_TRIM);
    }

    let cmdset = id.word(offset_of!(AtaIdentify, cmdset82) / 2);

    // Enable write cache if supported
    if cmdset & ATA_IDENTIFY_WRITECACHE != 0 {
        // We don't care if it fails.
        let _ = atascsi_port_set_features(ap, ATA_SF_WRITECACHE_EN, 0);
    }

    // Enable read lookahead if supported
    if cmdset & ATA_IDENTIFY_LOOKAHEAD != 0 {
        // We don't care if it fails.
        let _ = atascsi_port_set_features(ap, ATA_SF_LOOKAHEAD_EN, 0);
    }

    // FREEZE LOCK the device so malicious users can't lock it on us. As there is no harm in
    // issuing this to devices that don't support the security feature set we just send it,
    // and don't bother checking if the device sends a command abort to tell us it doesn't
    // support it
    let xa = atascsi_port_xa(ahp);
    xa.with_fis(|fis| {
        fis.command = ATA_C_SEC_FREEZE_LOCK;
        fis.flags = ap.pmp_flags();
    });
    xa.flags.set(ATA_F_POLL);
    xa.timeout.set(1000);
    xa.complete.set(Some(ata_polled_complete));
    xa.pmp_port.set(ap.ap_pmp_port);
    xa.atascsi_private.set(AtaPrivate::Iopool(&ahp.ahp_iopool));
    ata_exec(as_, xa);
    let _ = ata_polled(xa); // we don't care if it doesn't work

    Ok(())
}

/// `scsi_io_get(&ahp->ahp_iopool, SCSI_NOSLEEP)` and the C's "no free xfers on a new port"
/// panic.
fn atascsi_port_xa(ahp: &'static AtascsiHostPort) -> &'static AtaXfer {
    let Some(io) = scsi_io_get(&ahp.ahp_iopool, SCSI_NOSLEEP) else {
        panic(format_args!("no free xfers on a new port"));
    };
    // SAFETY: an opening of the host port's pool: a controller ata_xfer.
    unsafe { io.cast::<AtaXfer>().as_ref() }
}

/// `atascsi_free`: the adapter's `dev_free`: forgets a port, and its host port once the last
/// LUN is gone.
pub fn atascsi_free(link: &'static ScsiLink) {
    let as_ = atascsi_link_as(link);

    let port = i32::from(link.target.get());
    if link.target.get() >= link.bus().sb_adapter_buswidth.get() {
        return;
    }

    let Some(slot) = as_.host_port_slot(port as usize) else {
        return;
    };
    let Some(ahpp) = slot.get() else {
        return;
    };
    // SAFETY: a host port atascsi_probe allocated, still in its slot.
    let ahp = unsafe { ahpp.as_ref() };

    let lun = i32::from(link.lun.get());
    if lun >= ahp.ahp_nports {
        return;
    }

    if let Some(pslot) = ahp.port_slot(lun as usize)
        && let Some(app) = pslot.take()
    {
        free(app.cast(), M_DEVBUF, size_of::<AtascsiPort>());
    }

    // SAFETY: `as_cookie` is the controller's `aaa_cookie`.
    unsafe { (as_.as_methods.ata_free)(as_.as_cookie, port, lun) };

    if lun == ahp.ahp_nports - 1 {
        // we've already freed all of ahp->ahp_ports, now free ahp itself. this relies on the
        // order luns are detached in scsi_detach_target().
        let nports = ahp.ahp_nports.max(0) as usize;
        if let Some(ports) = NonNull::new(ahp.ahp_ports.cast_mut()) {
            free(
                ports.cast(),
                M_DEVBUF,
                nports * size_of::<Cell<Option<NonNull<AtascsiPort>>>>(),
            );
        }
        slot.set(None);
        free(ahpp.cast(), M_DEVBUF, size_of::<AtascsiHostPort>());
    }
}

/// `atascsi_cmd`: the adapter's `scsi_cmd`.
pub fn atascsi_cmd(xs: &'static ScsiXfer) {
    let link = xs.link();

    let Some(ap) = atascsi_lookup_port(link) else {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    };

    match ap.ap_type {
        ATA_PORT_T_DISK => atascsi_disk_cmd(xs),
        ATA_PORT_T_ATAPI => atascsi_atapi_cmd(xs),
        ATA_PORT_T_PM => atascsi_pmp_cmd(xs),

        // ATA_PORT_T_NONE
        _ => atascsi_done(xs, XS_DRIVER_STUFFUP),
    }
}

/// The FIS of a read or write of `sector_count` blocks at `lba` by the transfer in slot
/// `tag`: NCQ when `ncq`, else LBA48 when the transfer needs it, else LBA28. `fis` keeps the
/// members the C does not set.
pub fn atascsi_disk_rw_fis(
    fis: &mut AtaFisH2d,
    pmp_flags: u8,
    write: bool,
    ncq: bool,
    tag: u8,
    lba: u64,
    sector_count: u32,
) {
    fis.flags = pmp_flags;
    fis.lba_low = (lba & 0xff) as u8;
    fis.lba_mid = ((lba >> 8) & 0xff) as u8;
    fis.lba_high = ((lba >> 16) & 0xff) as u8;

    if ncq {
        // Use NCQ
        fis.command = if write {
            ATA_C_WRITE_FPDMA
        } else {
            ATA_C_READ_FPDMA
        };
        fis.device = ATA_H2D_DEVICE_LBA;
        fis.lba_low_exp = ((lba >> 24) & 0xff) as u8;
        fis.lba_mid_exp = ((lba >> 32) & 0xff) as u8;
        fis.lba_high_exp = ((lba >> 40) & 0xff) as u8;
        fis.sector_count = tag << 3;
        fis.features = (sector_count & 0xff) as u8;
        fis.features_exp = ((sector_count >> 8) & 0xff) as u8;
    } else if sector_count > 0x100 || lba > 0xfffffff {
        // Use LBA48
        fis.command = if write {
            ATA_C_WRITEDMA_EXT
        } else {
            ATA_C_READDMA_EXT
        };
        fis.device = ATA_H2D_DEVICE_LBA;
        fis.lba_low_exp = ((lba >> 24) & 0xff) as u8;
        fis.lba_mid_exp = ((lba >> 32) & 0xff) as u8;
        fis.lba_high_exp = ((lba >> 40) & 0xff) as u8;
        fis.sector_count = (sector_count & 0xff) as u8;
        fis.sector_count_exp = ((sector_count >> 8) & 0xff) as u8;
    } else {
        // Use LBA
        fis.command = if write { ATA_C_WRITEDMA } else { ATA_C_READDMA };
        fis.device = ATA_H2D_DEVICE_LBA | ((lba >> 24) & 0x0f) as u8;
        fis.sector_count = (sector_count & 0xff) as u8;
    }
}

/// `atascsi_disk_cmd`: a command for a disk.
pub fn atascsi_disk_cmd(xs: &'static ScsiXfer) {
    let link = xs.link();
    let as_ = atascsi_link_as(link);
    let Some(ap) = atascsi_lookup_port(link) else {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    };
    let xa = atascsi_xs_xa(xs);

    let flags = match xs.cmd.get().opcode {
        READ_COMMAND | READ_10 | READ_12 | READ_16 => ATA_F_READ,
        // deal with io outside the switch
        WRITE_COMMAND | WRITE_10 | WRITE_12 | WRITE_16 => ATA_F_WRITE,

        WRITE_SAME_16 => {
            atascsi_disk_write_same_16(xs);
            return;
        }
        UNMAP => {
            atascsi_disk_unmap(xs);
            return;
        }

        SYNCHRONIZE_CACHE => {
            atascsi_disk_sync(xs);
            return;
        }
        REQUEST_SENSE => {
            atascsi_disk_sense(xs);
            return;
        }
        INQUIRY => {
            atascsi_disk_inq(xs);
            return;
        }
        READ_CAPACITY => {
            atascsi_disk_capacity(xs);
            return;
        }
        READ_CAPACITY_16 => {
            atascsi_disk_capacity16(xs);
            return;
        }

        ATA_PASSTHRU_12 => {
            atascsi_passthru_12(xs);
            return;
        }
        ATA_PASSTHRU_16 => {
            atascsi_passthru_16(xs);
            return;
        }

        START_STOP => {
            atascsi_disk_start_stop(xs);
            return;
        }

        TEST_UNIT_READY | PREVENT_ALLOW => {
            atascsi_done(xs, XS_NOERROR);
            return;
        }

        _ => {
            atascsi_done(xs, XS_DRIVER_STUFFUP);
            return;
        }
    };

    xa.flags.set(flags);
    let (lba, sector_count) = scsi_cmd_rw_decode(&xs.cmd.get());
    if (lba >> 48) != 0 || (sector_count >> 16) != 0 {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let ncq = ap.ap_features.get() & ATA_PORT_F_NCQ != 0
        && i32::from(xa.tag.get()) < ap.ap_ncqdepth.get()
        && xs.flags.get() & SCSI_POLL == 0;
    if ncq {
        xa.flags.set(xa.flags.get() | ATA_F_NCQ);
    }
    xa.with_fis(|fis| {
        atascsi_disk_rw_fis(
            fis,
            ap.pmp_flags(),
            flags & ATA_F_WRITE != 0,
            ncq,
            xa.tag.get(),
            lba,
            sector_count,
        )
    });

    xa.data.set(xs.data());
    xa.datalen.set(xs.datalen().max(0) as usize);
    xa.complete.set(Some(atascsi_disk_cmd_done));
    xa.timeout.set(xs.timeout.get().max(0) as u32);
    xa.pmp_port.set(ap.ap_pmp_port);
    xa.atascsi_private.set(AtaPrivate::Xs(xs));
    if xs.flags.get() & SCSI_POLL != 0 {
        xa.flags.set(xa.flags.get() | ATA_F_POLL);
    }

    ata_exec(as_, xa);
}

/// The `XS_*` of a finished command, and the C's panic on any other state (`what` names the
/// completion).
fn atascsi_xs_error(xa: &AtaXfer, what: &str) -> i32 {
    match xa.state() {
        ATA_S_COMPLETE => XS_NOERROR,
        // fake sense?
        ATA_S_ERROR => XS_DRIVER_STUFFUP,
        ATA_S_TIMEOUT => XS_TIMEOUT,
        s => panic(format_args!("{what}: unexpected ata_xfer state ({s})")),
    }
}

/// `atascsi_disk_cmd_done`.
pub fn atascsi_disk_cmd_done(xa: &'static AtaXfer) {
    let xs = xa.xs();

    xs.error.set(atascsi_xs_error(xa, "atascsi_disk_cmd_done"));

    xs.resid.set(xa.resid.get());

    scsi_done(xs);
}

/// `atascsi_disk_inq`: INQUIRY, standard or a VPD page.
pub fn atascsi_disk_inq(xs: &'static ScsiXfer) {
    let inq: ScsiInquiry = xs.cmd_as();

    if xs.cmdlen.get() as usize != size_of::<ScsiInquiry>() {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    if inq.flags & SI_EVPD != 0 {
        match inq.pagecode {
            SI_PG_SUPPORTED => atascsi_disk_vpd_supported(xs),
            SI_PG_SERIAL => atascsi_disk_vpd_serial(xs),
            SI_PG_DEVID => atascsi_disk_vpd_ident(xs),
            SI_PG_ATA => atascsi_disk_vpd_ata(xs),
            SI_PG_DISK_LIMITS => atascsi_disk_vpd_limits(xs),
            SI_PG_DISK_INFO => atascsi_disk_vpd_info(xs),
            SI_PG_DISK_THIN => atascsi_disk_vpd_thin(xs),
            _ => atascsi_done(xs, XS_DRIVER_STUFFUP),
        }
    } else {
        atascsi_disk_inquiry(xs);
    }
}

/// The port of a transfer's link; a missing one completes the transfer with
/// `XS_DRIVER_STUFFUP` (see the module's deviations).
fn atascsi_xs_port(xs: &'static ScsiXfer) -> Option<&'static AtascsiPort> {
    let ap = atascsi_lookup_port(xs.link());
    if ap.is_none() {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
    }
    ap
}

/// `bcopy(&pg, xs->data, MIN(sizeof(pg), xs->datalen))`.
fn atascsi_copy_reply(xs: &ScsiXfer, reply: &[u8]) {
    // SAFETY: the adapter owns the transfer between scsi_cmd and scsi_done.
    let data = unsafe { xs.data_slice() };
    let n = min(reply.len(), data.len());
    data[..n].copy_from_slice(&reply[..n]);
}

/// The standard INQUIRY data of an ATA disk with IDENTIFY data `id`.
pub fn atascsi_inquiry_data(id: &AtaIdentify) -> ScsiInquiryData {
    let mut inq = ScsiInquiryData::zeroed();

    inq.device = T_DIRECT;
    inq.version = SCSI_REV_SPC3;
    inq.response_format = SID_SCSI2_RESPONSE;
    inq.additional_length = SID_SCSI2_ALEN as u8;
    inq.flags |= SID_CmdQue;
    inq.vendor.copy_from_slice(b"ATA     ");
    ata_swapcopy(&id.model, &mut inq.product);
    ata_swapcopy(&id.firmware, &mut inq.revision);

    inq
}

/// `atascsi_disk_inquiry`: the standard INQUIRY data.
pub fn atascsi_disk_inquiry(xs: &'static ScsiXfer) {
    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };

    let inq = atascsi_inquiry_data(ap.identify());

    scsi_copy_internal_data(xs, inq.as_bytes());

    atascsi_done(xs, XS_NOERROR);
}

/// `atascsi_disk_vpd_supported`: VPD page 0, the supported pages.
pub fn atascsi_disk_vpd_supported(xs: &'static ScsiXfer) {
    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    let fat = if ap.ap_features.get() & ATA_PORT_F_TRIM != 0 {
        0
    } else {
        1
    };

    // struct { struct scsi_vpd_hdr hdr; u_int8_t list[7]; }
    let mut pg = [0u8; size_of::<ScsiVpdHdr>() + 7];
    let mut hdr = ScsiVpdHdr::zeroed();
    hdr.device = T_DIRECT;
    hdr.page_code = SI_PG_SUPPORTED;
    _lto2b(7 - fat as u32, &mut hdr.page_length);
    pg[..size_of::<ScsiVpdHdr>()].copy_from_slice(hdr.as_bytes());
    pg[size_of::<ScsiVpdHdr>()..].copy_from_slice(&[
        SI_PG_SUPPORTED,
        SI_PG_SERIAL,
        SI_PG_DEVID,
        SI_PG_ATA,
        SI_PG_DISK_LIMITS,
        SI_PG_DISK_INFO,
        SI_PG_DISK_THIN, // "trimmed" if fat. get it? tehe.
    ]);

    atascsi_copy_reply(xs, &pg[..pg.len() - fat]);

    atascsi_done(xs, XS_NOERROR);
}

/// `atascsi_disk_vpd_serial`: VPD page 80h, the serial number.
pub fn atascsi_disk_vpd_serial(xs: &'static ScsiXfer) {
    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    let id = ap.identify();
    let mut pg = ScsiVpdSerial::zeroed();

    pg.hdr.device = T_DIRECT;
    pg.hdr.page_code = SI_PG_SERIAL;
    _lto2b(id.serial.len() as u32, &mut pg.hdr.page_length);
    ata_swapcopy(&id.serial, &mut pg.serial[..id.serial.len()]);

    atascsi_copy_reply(xs, pg.as_bytes());

    atascsi_done(xs, XS_NOERROR);
}

/// The device identification VPD page (83h) of an ATA disk: its WWN when it reports one,
/// else a T10 vendor identifier made of "ATA", the model and the serial number. Returns the
/// page and its length.
pub fn atascsi_vpd_ident_page(id: &AtaIdentify) -> ([u8; 76], usize) {
    // struct { struct scsi_vpd_hdr hdr; struct scsi_vpd_devid_hdr devid_hdr;
    //     u_int8_t devid[68]; }
    let mut pg = [0u8; 76];
    let hdrs = size_of::<ScsiVpdHdr>() + size_of::<ScsiVpdDevidHdr>();
    let mut devid_hdr = ScsiVpdDevidHdr::zeroed();
    let mut pg_len;

    if id.word(offset_of!(AtaIdentify, features87) / 2) & ATA_ID_F87_WWN != 0 {
        pg_len = 8;

        devid_hdr.pi_code = VPD_DEVID_CODE_BINARY;
        devid_hdr.flags = VPD_DEVID_ASSOC_LU | VPD_DEVID_TYPE_NAA;

        let off = offset_of!(AtaIdentify, naa_ieee_oui);
        ata_swapcopy(
            &id.as_bytes()[off..off + pg_len],
            &mut pg[hdrs..hdrs + pg_len],
        );
    } else {
        pg_len = 68;

        devid_hdr.pi_code = VPD_DEVID_CODE_ASCII;
        devid_hdr.flags = VPD_DEVID_ASSOC_LU | VPD_DEVID_TYPE_T10;

        let p = &mut pg[hdrs..];
        p[..8].copy_from_slice(b"ATA     ");
        ata_swapcopy(&id.model, &mut p[8..8 + id.model.len()]);
        let p = &mut p[8 + id.model.len()..];
        ata_swapcopy(&id.serial, &mut p[..id.serial.len()]);
    }

    devid_hdr.len = pg_len as u8;
    pg_len += size_of::<ScsiVpdDevidHdr>();

    let mut hdr = ScsiVpdHdr::zeroed();
    hdr.device = T_DIRECT;
    hdr.page_code = SI_PG_DEVID;
    _lto2b(pg_len as u32, &mut hdr.page_length);
    pg_len += size_of::<ScsiVpdHdr>();

    pg[..size_of::<ScsiVpdHdr>()].copy_from_slice(hdr.as_bytes());
    pg[size_of::<ScsiVpdHdr>()..hdrs].copy_from_slice(devid_hdr.as_bytes());

    (pg, pg_len)
}

/// `atascsi_disk_vpd_ident`: VPD page 83h, the device identification.
pub fn atascsi_disk_vpd_ident(xs: &'static ScsiXfer) {
    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };

    let (pg, pg_len) = atascsi_vpd_ident_page(ap.identify());

    atascsi_copy_reply(xs, &pg[..pg_len]);

    atascsi_done(xs, XS_NOERROR);
}

/// `memset(dst, ' ', sizeof(dst)); memcpy(dst, src, MIN(strlen(src), sizeof(dst)))`.
fn atascsi_pad(dst: &mut [u8], src: &[u8]) {
    dst.fill(b' ');
    let n = min(src.len(), dst.len());
    dst[..n].copy_from_slice(&src[..n]);
}

/// `atascsi_disk_vpd_ata`: VPD page 89h, the ATA information.
pub fn atascsi_disk_vpd_ata(xs: &'static ScsiXfer) {
    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    let mut pg = ScsiVpdAta::zeroed();

    pg.hdr.device = T_DIRECT;
    pg.hdr.page_code = SI_PG_ATA;
    _lto2b(
        (size_of::<ScsiVpdAta>() - size_of::<ScsiVpdHdr>()) as u32,
        &mut pg.hdr.page_length,
    );

    atascsi_pad(&mut pg.sat_vendor, b"OpenBSD");
    atascsi_pad(&mut pg.sat_product, b"atascsi");
    atascsi_pad(&mut pg.sat_revision, OSRELEASE.as_bytes());

    // XXX device signature

    match ap.ap_type {
        ATA_PORT_T_DISK => pg.command_code = VPD_ATA_COMMAND_CODE_ATA,
        ATA_PORT_T_ATAPI => pg.command_code = VPD_ATA_COMMAND_CODE_ATAPI,
        _ => {}
    }

    pg.identify.copy_from_slice(ap.identify().as_bytes());

    atascsi_copy_reply(xs, pg.as_bytes());

    atascsi_done(xs, XS_NOERROR);
}

/// `atascsi_disk_vpd_limits`: VPD page B0h, the block limits.
pub fn atascsi_disk_vpd_limits(xs: &'static ScsiXfer) {
    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    let mut pg = ScsiVpdDiskLimits::zeroed();

    pg.hdr.device = T_DIRECT;
    pg.hdr.page_code = SI_PG_DISK_LIMITS;
    _lto2b(
        u32::from(SI_PG_DISK_LIMITS_LEN_THIN),
        &mut pg.hdr.page_length,
    );

    _lto2b(
        1 << ata_identify_block_l2p_exp(ap.identify()),
        &mut pg.optimal_xfer_granularity,
    );

    if ap.ap_features.get() & ATA_PORT_F_TRIM != 0 {
        // ATA only supports 65535 blocks per TRIM descriptor, so avoid having to split UNMAP
        // descriptors and overflow the page limit by using that as a max.
        _lto4b(ATA_DSM_TRIM_MAX_LEN, &mut pg.max_unmap_lba_count);
        _lto4b(512 / 8, &mut pg.max_unmap_desc_count);
    }

    atascsi_copy_reply(xs, pg.as_bytes());

    atascsi_done(xs, XS_NOERROR);
}

/// `atascsi_disk_vpd_info`: VPD page B1h, the block device characteristics.
pub fn atascsi_disk_vpd_info(xs: &'static ScsiXfer) {
    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    let id = ap.identify();
    let mut pg = ScsiVpdDiskInfo::zeroed();

    pg.hdr.device = T_DIRECT;
    pg.hdr.page_code = SI_PG_DISK_INFO;
    _lto2b(
        (size_of::<ScsiVpdDiskInfo>() - size_of::<ScsiVpdHdr>()) as u32,
        &mut pg.hdr.page_length,
    );

    _lto2b(
        u32::from(id.word(offset_of!(AtaIdentify, rpm) / 2)),
        &mut pg.rpm,
    );
    pg.form_factor = (id.word(offset_of!(AtaIdentify, form) / 2) & ATA_ID_FORM_MASK) as u8;

    atascsi_copy_reply(xs, pg.as_bytes());

    atascsi_done(xs, XS_NOERROR);
}

/// `atascsi_disk_vpd_thin`: VPD page B2h, logical block provisioning (TRIM).
pub fn atascsi_disk_vpd_thin(xs: &'static ScsiXfer) {
    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    if ap.ap_features.get() & ATA_PORT_F_TRIM == 0 {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let mut pg = ScsiVpdDiskThin::zeroed();
    pg.hdr.device = T_DIRECT;
    pg.hdr.page_code = SI_PG_DISK_THIN;
    _lto2b(
        (size_of::<ScsiVpdDiskThin>() - size_of::<ScsiVpdHdr>()) as u32,
        &mut pg.hdr.page_length,
    );

    pg.flags = VPD_DISK_THIN_TPU | VPD_DISK_THIN_TPWS;

    atascsi_copy_reply(xs, pg.as_bytes());

    atascsi_done(xs, XS_NOERROR);
}

/// `atascsi_disk_write_same_16`: WRITE SAME(16) with UNMAP, as one TRIM range.
pub fn atascsi_disk_write_same_16(xs: &'static ScsiXfer) {
    let link = xs.link();
    let as_ = atascsi_link_as(link);
    let xa = atascsi_xs_xa(xs);

    if xs.cmdlen.get() as usize != size_of::<ScsiWriteSame16>() {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    let cdb: ScsiWriteSame16 = xs.cmd_as();

    if cdb.flags & WRITE_SAME_F_UNMAP == 0 || ap.ap_features.get() & ATA_PORT_F_TRIM == 0 {
        // generate sense data
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    if xs.datalen() < 512 {
        // generate sense data
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let lba = _8btol(&cdb.lba);
    let length = _4btol(&cdb.length);

    if length > ATA_DSM_TRIM_MAX_LEN {
        // XXX we dont support requests over 65535 blocks
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    xa.data.set(xs.data());
    xa.datalen.set(512);
    xa.flags.set(ATA_F_WRITE);
    xa.pmp_port.set(ap.ap_pmp_port);
    if xs.flags.get() & SCSI_POLL != 0 {
        xa.flags.set(xa.flags.get() | ATA_F_POLL);
    }
    xa.complete.set(Some(atascsi_disk_write_same_16_done));
    xa.atascsi_private.set(AtaPrivate::Xs(xs));
    xa.timeout.set(xs.timeout.get().max(45000) as u32);

    // TRIM sends a list of blocks to discard in the databuf.
    // SAFETY: the adapter owns the transfer between scsi_cmd and scsi_done; it has at least
    // 512 bytes.
    let data = unsafe { xs.data_slice() };
    data[..512].fill(0);
    data[..8].copy_from_slice(&ata_dsm_trim_desc(lba, length).to_le_bytes());

    xa.with_fis(|fis| {
        fis.flags = ap.pmp_flags();
        fis.command = ATA_C_DSM;
        fis.features = ATA_DSM_TRIM;
        fis.sector_count = 1;
    });

    ata_exec(as_, xa);
}

/// `atascsi_disk_write_same_16_done`.
pub fn atascsi_disk_write_same_16_done(xa: &'static AtaXfer) {
    let xs = xa.xs();

    xs.error
        .set(atascsi_xs_error(xa, "atascsi_disk_write_same_16_done"));

    scsi_done(xs);
}

/// `atascsi_disk_unmap`: UNMAP, checked here and turned into TRIM ranges by
/// `atascsi_disk_unmap_task` (on the system task queue when the transfer may not sleep).
pub fn atascsi_disk_unmap(xs: &'static ScsiXfer) {
    let xa = atascsi_xs_xa(xs);

    if xs.flags.get() & SCSI_POLL != 0 || xs.cmdlen.get() as usize != size_of::<ScsiUnmap>() {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let cdb: ScsiUnmap = xs.cmd_as();
    let len = _2btol(&cdb.list_len) as usize;
    if xs.datalen().max(0) as usize != len || len < size_of::<ScsiUnmapData>() {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    // SAFETY: the adapter owns the transfer between scsi_cmd and scsi_done.
    let data = unsafe { xs.data_slice() };
    let unmap: &ScsiUnmapData = wire_ref(data);
    if _2btol(&unmap.data_length) as usize != len {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let len = _2btol(&unmap.desc_length) as usize;
    if len != xs.datalen() as usize - size_of::<ScsiUnmapData>() {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    if len < size_of::<ScsiUnmapDesc>() {
        // no work, no error according to sbc3
        atascsi_done(xs, XS_NOERROR);
        return;
    }

    if len > size_of::<ScsiUnmapDesc>() * 64 {
        // more work than we advertised
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    // let's go
    let arg = ptr::from_ref(xs).cast_mut().cast();
    if xs.flags.get() & SCSI_NOSLEEP != 0 {
        task_set(&xa.task, atascsi_disk_unmap_task, arg);
        task_add(SYSTQ, &xa.task);
    } else {
        // we can already sleep for memory
        atascsi_disk_unmap_task(arg);
    }
}

/// `atascsi_disk_unmap_task`: builds the TRIM ranges of an UNMAP and issues them.
pub fn atascsi_disk_unmap_task(xxs: *mut c_void) {
    // SAFETY: atascsi_disk_unmap passes the transfer it was given, which stays the adapter's
    // until scsi_done.
    let xs: &'static ScsiXfer = unsafe { &*xxs.cast::<ScsiXfer>().cast_const() };
    let link = xs.link();
    let as_ = atascsi_link_as(link);
    let xa = atascsi_xs_xa(xs);

    let Some(trims) = malloc(512, M_DEVBUF, M_WAITOK | M_ZERO) else {
        panic(format_args!(
            "atascsi_disk_unmap_task: dma_alloc(PR_WAITOK) failed"
        ));
    };

    let Some(ap) = atascsi_lookup_port(link) else {
        free(trims, M_DEVBUF, 512);
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    };
    // SAFETY: the adapter owns the transfer between scsi_cmd and scsi_done.
    let data = unsafe { xs.data_slice() };
    let unmap: &ScsiUnmapData = wire_ref(data);
    let descs = &data[size_of::<ScsiUnmapData>()..];

    let len = _2btol(&unmap.desc_length) as usize / size_of::<ScsiUnmapDesc>();
    for i in 0..len {
        let d: &ScsiUnmapDesc = wire_ref(&descs[i * size_of::<ScsiUnmapDesc>()..]);
        if _4btol(&d.logical_blocks) > ATA_DSM_TRIM_MAX_LEN {
            // fail:
            free(trims, M_DEVBUF, 512);
            atascsi_done(xs, XS_DRIVER_STUFFUP);
            return;
        }

        let desc = ata_dsm_trim_desc(_8btol(&d.logical_addr), _4btol(&d.logical_blocks));
        // SAFETY: at most 64 descriptors (atascsi_disk_unmap checked), so `i * 8 + 8` stays
        // within the 512 bytes of `trims`.
        unsafe {
            ptr::copy_nonoverlapping(desc.to_le_bytes().as_ptr(), trims.as_ptr().add(i * 8), 8)
        };
    }

    xa.data.set(trims.as_ptr());
    xa.datalen.set(512);
    xa.flags.set(ATA_F_WRITE);
    xa.pmp_port.set(ap.ap_pmp_port);
    xa.complete.set(Some(atascsi_disk_unmap_done));
    xa.atascsi_private.set(AtaPrivate::Xs(xs));
    xa.timeout.set(xs.timeout.get().max(45000) as u32);

    xa.with_fis(|fis| {
        fis.flags = ap.pmp_flags();
        fis.command = ATA_C_DSM;
        fis.features = ATA_DSM_TRIM;
        fis.sector_count = 1;
    });

    ata_exec(as_, xa);
}

/// `atascsi_disk_unmap_done`.
pub fn atascsi_disk_unmap_done(xa: &'static AtaXfer) {
    let xs = xa.xs();

    if let Some(trims) = NonNull::new(xa.data.get()) {
        free(trims, M_DEVBUF, 512);
    }

    xs.error
        .set(atascsi_xs_error(xa, "atascsi_disk_unmap_done"));

    scsi_done(xs);
}

/// `atascsi_disk_sync`: SYNCHRONIZE CACHE as FLUSH CACHE.
pub fn atascsi_disk_sync(xs: &'static ScsiXfer) {
    let link = xs.link();
    let as_ = atascsi_link_as(link);
    let xa = atascsi_xs_xa(xs);

    if xs.cmdlen.get() as usize != size_of::<ScsiSynchronizeCache>() {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    xa.datalen.set(0);
    xa.flags.set(ATA_F_READ);
    xa.complete.set(Some(atascsi_disk_sync_done));
    // Spec says flush cache can take >30 sec, so give it at least 45.
    xa.timeout.set(xs.timeout.get().max(45000) as u32);
    xa.atascsi_private.set(AtaPrivate::Xs(xs));
    xa.pmp_port.set(ap.ap_pmp_port);
    if xs.flags.get() & SCSI_POLL != 0 {
        xa.flags.set(xa.flags.get() | ATA_F_POLL);
    }

    xa.with_fis(|fis| {
        fis.flags = ap.pmp_flags();
        fis.command = ATA_C_FLUSH_CACHE;
        fis.device = 0;
    });

    ata_exec(as_, xa);
}

/// `atascsi_disk_sync_done`.
pub fn atascsi_disk_sync_done(xa: &'static AtaXfer) {
    let xs = xa.xs();

    match xa.state() {
        ATA_S_COMPLETE => xs.error.set(XS_NOERROR),

        s @ (ATA_S_ERROR | ATA_S_TIMEOUT) => {
            printf(format_args!(
                "atascsi_disk_sync_done: {}\n",
                if s == ATA_S_TIMEOUT {
                    "timeout"
                } else {
                    "error"
                }
            ));
            xs.error.set(if s == ATA_S_TIMEOUT {
                XS_TIMEOUT
            } else {
                XS_DRIVER_STUFFUP
            });
        }

        s => panic(format_args!(
            "atascsi_disk_sync_done: unexpected ata_xfer state ({s})"
        )),
    }

    scsi_done(xs);
}

/// `ata_identify_blocks`: the last LBA (the C's `blocks - 1`).
pub fn ata_identify_blocks(id: &AtaIdentify) -> u64 {
    let mut blocks: u64 = 0;

    if id.word(offset_of!(AtaIdentify, cmdset83) / 2) & 0x0400 != 0 {
        // LBA48 feature set supported
        let base = offset_of!(AtaIdentify, addrsecxt) / 2;
        for i in (0..4).rev() {
            blocks <<= 16;
            blocks += u64::from(id.word(base + i));
        }
    } else {
        let base = offset_of!(AtaIdentify, addrsec) / 2;
        blocks = u64::from(id.word(base + 1));
        blocks <<= 16;
        blocks += u64::from(id.word(base));
    }

    blocks.wrapping_sub(1)
}

/// `ata_identify_blocksize`: the logical sector size.
pub fn ata_identify_blocksize(id: &AtaIdentify) -> u32 {
    let mut blocksize: u32 = 512;
    let p2l_sect = id.word(offset_of!(AtaIdentify, p2l_sect) / 2);

    if (p2l_sect & ATA_ID_P2L_SECT_MASK) == ATA_ID_P2L_SECT_VALID
        && p2l_sect & ATA_ID_P2L_SECT_SIZESET != 0
    {
        let base = offset_of!(AtaIdentify, words_lsec) / 2;
        blocksize = u32::from(id.word(base + 1));
        blocksize <<= 16;
        blocksize += u32::from(id.word(base));
        blocksize <<= 1;
    }

    blocksize
}

/// `ata_identify_block_l2p_exp`: logical sectors per physical sector, as a shift.
pub fn ata_identify_block_l2p_exp(id: &AtaIdentify) -> u32 {
    let mut exponent = 0;
    let p2l_sect = id.word(offset_of!(AtaIdentify, p2l_sect) / 2);

    if (p2l_sect & ATA_ID_P2L_SECT_MASK) == ATA_ID_P2L_SECT_VALID
        && p2l_sect & ATA_ID_P2L_SECT_SET != 0
    {
        exponent = u32::from(p2l_sect & ATA_ID_P2L_SECT_SIZE);
    }

    exponent
}

/// `ata_identify_block_logical_align`: the offset of the first logical sector in its
/// physical sector.
pub fn ata_identify_block_logical_align(id: &AtaIdentify) -> u32 {
    let mut align = 0;
    let p2l_sect = id.word(offset_of!(AtaIdentify, p2l_sect) / 2);
    let logical_align = id.word(offset_of!(AtaIdentify, logical_align) / 2);

    if (p2l_sect & ATA_ID_P2L_SECT_MASK) == ATA_ID_P2L_SECT_VALID
        && p2l_sect & ATA_ID_P2L_SECT_SET != 0
        && (logical_align & ATA_ID_LALIGN_MASK) == ATA_ID_LALIGN_VALID
    {
        align = u32::from(logical_align & ATA_ID_LALIGN);
    }

    align
}

/// `atascsi_disk_capacity`: READ CAPACITY(10).
pub fn atascsi_disk_capacity(xs: &'static ScsiXfer) {
    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    if xs.cmdlen.get() as usize != size_of::<ScsiReadCapacity>() {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let mut rcd = ScsiReadCapData::zeroed();
    let capacity = ata_identify_blocks(ap.identify()).min(0xffffffff);

    _lto4b(capacity as u32, &mut rcd.addr);
    _lto4b(ata_identify_blocksize(ap.identify()), &mut rcd.length);

    atascsi_copy_reply(xs, rcd.as_bytes());

    atascsi_done(xs, XS_NOERROR);
}

/// The READ CAPACITY(16) data of a port.
pub fn atascsi_read_cap_data_16(id: &AtaIdentify, features: i32) -> ScsiReadCapData16 {
    let mut rcd = ScsiReadCapData16::zeroed();
    let mut lowest_aligned: u16 = 0;

    _lto8b(ata_identify_blocks(id), &mut rcd.addr);
    _lto4b(ata_identify_blocksize(id), &mut rcd.length);
    rcd.logical_per_phys = ata_identify_block_l2p_exp(id) as u8;
    let align = ata_identify_block_logical_align(id);
    if align > 0 {
        lowest_aligned = ((1u32 << rcd.logical_per_phys).wrapping_sub(align)) as u16;
    }

    if features & ATA_PORT_F_TRIM != 0 {
        lowest_aligned |= READ_CAP_16_TPE;

        if id.word(offset_of!(AtaIdentify, add_support) / 2) & ATA_ID_ADD_SUPPORT_DRT != 0 {
            lowest_aligned |= READ_CAP_16_TPRZ;
        }
    }
    _lto2b(u32::from(lowest_aligned), &mut rcd.lowest_aligned);

    rcd
}

/// `atascsi_disk_capacity16`: READ CAPACITY(16).
pub fn atascsi_disk_capacity16(xs: &'static ScsiXfer) {
    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    if xs.cmdlen.get() as usize != size_of::<ScsiReadCapacity16>() {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let rcd = atascsi_read_cap_data_16(ap.identify(), ap.ap_features.get());

    atascsi_copy_reply(xs, rcd.as_bytes());

    atascsi_done(xs, XS_NOERROR);
}

/// `atascsi_passthru_map`: the transfer's data and flags for an ATA PASS-THROUGH; `false`
/// for a protocol not supported yet (the C's 1).
pub fn atascsi_passthru_map(xs: &'static ScsiXfer, count_proto: u8, _flags: u8) -> bool {
    let xa = atascsi_xs_xa(xs);

    xa.data.set(xs.data());
    xa.datalen.set(xs.datalen().max(0) as usize);
    xa.timeout.set(xs.timeout.get().max(0) as u32);
    let mut flags = 0;
    if xs.flags.get() & SCSI_DATA_IN != 0 {
        flags |= ATA_F_READ;
    }
    if xs.flags.get() & SCSI_DATA_OUT != 0 {
        flags |= ATA_F_WRITE;
    }
    if xs.flags.get() & SCSI_POLL != 0 {
        flags |= ATA_F_POLL;
    }

    match count_proto & ATA_PASSTHRU_PROTO_MASK {
        ATA_PASSTHRU_PROTO_NON_DATA
        | ATA_PASSTHRU_PROTO_PIO_DATAIN
        | ATA_PASSTHRU_PROTO_PIO_DATAOUT => flags |= ATA_F_PIO,
        _ => {
            // we dont support this yet
            xa.flags.set(flags);
            return false;
        }
    }
    xa.flags.set(flags);

    xa.atascsi_private.set(AtaPrivate::Xs(xs));
    xa.complete.set(Some(atascsi_passthru_done));

    true
}

/// `atascsi_passthru_12`: ATA PASS-THROUGH(12).
pub fn atascsi_passthru_12(xs: &'static ScsiXfer) {
    let link = xs.link();
    let as_ = atascsi_link_as(link);
    let xa = atascsi_xs_xa(xs);

    if xs.cmdlen.get() as usize != size_of::<ScsiAtaPassthru12>() {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let cdb: ScsiAtaPassthru12 = xs.cmd_as();
    // validate cdb

    if !atascsi_passthru_map(xs, cdb.count_proto, cdb.flags) {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    xa.with_fis(|fis| {
        fis.flags = ap.pmp_flags();
        fis.command = cdb.command;
        fis.features = cdb.features;
        fis.lba_low = cdb.lba_low;
        fis.lba_mid = cdb.lba_mid;
        fis.lba_high = cdb.lba_high;
        fis.device = cdb.device;
        fis.sector_count = cdb.sector_count;
    });
    xa.pmp_port.set(ap.ap_pmp_port);

    ata_exec(as_, xa);
}

/// `atascsi_passthru_16`: ATA PASS-THROUGH(16).
pub fn atascsi_passthru_16(xs: &'static ScsiXfer) {
    let link = xs.link();
    let as_ = atascsi_link_as(link);
    let xa = atascsi_xs_xa(xs);

    if xs.cmdlen.get() as usize != size_of::<ScsiAtaPassthru16>() {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let cdb: ScsiAtaPassthru16 = xs.cmd_as();
    // validate cdb

    if !atascsi_passthru_map(xs, cdb.count_proto, cdb.flags) {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    xa.with_fis(|fis| {
        fis.flags = ap.pmp_flags();
        fis.command = cdb.command;
        fis.features = cdb.features[1];
        fis.lba_low = cdb.lba_low[1];
        fis.lba_mid = cdb.lba_mid[1];
        fis.lba_high = cdb.lba_high[1];
        fis.device = cdb.device;
        fis.lba_low_exp = cdb.lba_low[0];
        fis.lba_mid_exp = cdb.lba_mid[0];
        fis.lba_high_exp = cdb.lba_high[0];
        fis.features_exp = cdb.features[0];
        fis.sector_count = cdb.sector_count[1];
        fis.sector_count_exp = cdb.sector_count[0];
    });
    xa.pmp_port.set(ap.ap_pmp_port);

    ata_exec(as_, xa);
}

/// `atascsi_passthru_done`.
pub fn atascsi_passthru_done(xa: &'static AtaXfer) {
    let xs = xa.xs();

    // XXX need to generate sense if cdb wants it

    match xa.state() {
        ATA_S_COMPLETE => xs.error.set(XS_NOERROR),
        ATA_S_ERROR => xs.error.set(XS_DRIVER_STUFFUP),
        ATA_S_TIMEOUT => {
            printf(format_args!("atascsi_passthru_done, timeout\n"));
            xs.error.set(XS_TIMEOUT);
        }
        s => panic(format_args!(
            "atascsi_atapi_cmd_done: unexpected ata_xfer state ({s})"
        )),
    }

    xs.resid.set(xa.resid.get());

    scsi_done(xs);
}

/// The REQUEST SENSE answer of `atascsi_disk_sense` and `atascsi_pmp_sense`: no sense.
fn atascsi_no_sense(xs: &'static ScsiXfer) {
    // SAFETY: the adapter owns the transfer between scsi_cmd and scsi_done.
    let data = unsafe { xs.data_slice() };
    data.fill(0);
    // check datalen > sizeof(struct scsi_sense_data)?
    if let Some(b) = data.get_mut(offset_of!(ScsiSenseData, error_code)) {
        *b = SSD_ERRCODE_CURRENT;
    }
    if let Some(b) = data.get_mut(offset_of!(ScsiSenseData, flags)) {
        *b = SKEY_NO_SENSE;
    }

    atascsi_done(xs, XS_NOERROR);
}

/// `atascsi_disk_sense`: REQUEST SENSE.
pub fn atascsi_disk_sense(xs: &'static ScsiXfer) {
    atascsi_no_sense(xs);
}

/// `atascsi_disk_start_stop`: START STOP UNIT; a stop is FLUSH CACHE, then STANDBY
/// IMMEDIATE.
pub fn atascsi_disk_start_stop(xs: &'static ScsiXfer) {
    let link = xs.link();
    let as_ = atascsi_link_as(link);
    let xa = atascsi_xs_xa(xs);

    if xs.cmdlen.get() as usize != size_of::<ScsiStartStop>() {
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let ss: ScsiStartStop = xs.cmd_as();
    if ss.how != SSS_STOP {
        atascsi_done(xs, XS_NOERROR);
        return;
    }

    // A SCSI START STOP UNIT command with the START bit set to zero gets translated into an
    // ATA FLUSH CACHE command followed by an ATA STANDBY IMMEDIATE command.
    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    xa.datalen.set(0);
    xa.flags.set(ATA_F_READ);
    xa.complete.set(Some(atascsi_disk_start_stop_done));
    // Spec says flush cache can take >30 sec, so give it at least 45.
    xa.timeout.set(xs.timeout.get().max(45000) as u32);
    xa.pmp_port.set(ap.ap_pmp_port);
    xa.atascsi_private.set(AtaPrivate::Xs(xs));
    if xs.flags.get() & SCSI_POLL != 0 {
        xa.flags.set(xa.flags.get() | ATA_F_POLL);
    }

    xa.with_fis(|fis| {
        fis.flags = ap.pmp_flags();
        fis.command = ATA_C_FLUSH_CACHE;
        fis.device = 0;
    });

    ata_exec(as_, xa);
}

/// `atascsi_disk_start_stop_done`: after FLUSH CACHE, STANDBY IMMEDIATE.
pub fn atascsi_disk_start_stop_done(xa: &'static AtaXfer) {
    let xs = xa.xs();
    let link = xs.link();
    let as_ = atascsi_link_as(link);

    match xa.state() {
        ATA_S_COMPLETE => {}

        s @ (ATA_S_ERROR | ATA_S_TIMEOUT) => {
            xs.error.set(if s == ATA_S_TIMEOUT {
                XS_TIMEOUT
            } else {
                XS_DRIVER_STUFFUP
            });
            xs.resid.set(xa.resid.get());
            scsi_done(xs);
            return;
        }

        s => panic(format_args!(
            "atascsi_disk_start_stop_done: unexpected ata_xfer state ({s})"
        )),
    }

    // The FLUSH CACHE command completed successfully; now issue the STANDBY IMMEDIATE
    // command.
    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    xa.datalen.set(0);
    xa.flags.set(ATA_F_READ);
    xa.set_state(ATA_S_SETUP);
    xa.complete.set(Some(atascsi_disk_cmd_done));
    // Spec says flush cache can take >30 sec, so give it at least 45.
    xa.timeout.set(xs.timeout.get().max(45000) as u32);
    xa.pmp_port.set(ap.ap_pmp_port);
    xa.atascsi_private.set(AtaPrivate::Xs(xs));
    if xs.flags.get() & SCSI_POLL != 0 {
        xa.flags.set(xa.flags.get() | ATA_F_POLL);
    }

    xa.with_fis(|fis| {
        fis.flags = ap.pmp_flags();
        fis.command = ATA_C_STANDBY_IMMED;
        fis.device = 0;
    });

    ata_exec(as_, xa);
}

/// `atascsi_atapi_cmd`: a command for an ATAPI device, sent through PACKET.
pub fn atascsi_atapi_cmd(xs: &'static ScsiXfer) {
    let link = xs.link();
    let as_ = atascsi_link_as(link);
    let xa = atascsi_xs_xa(xs);

    let mut flags = match xs.flags.get() & (SCSI_DATA_IN | SCSI_DATA_OUT) {
        SCSI_DATA_IN => ATA_F_PACKET | ATA_F_READ,
        SCSI_DATA_OUT => ATA_F_PACKET | ATA_F_WRITE,
        _ => ATA_F_PACKET,
    };
    flags |= ATA_F_GET_RFIS;
    xa.flags.set(flags);

    let Some(ap) = atascsi_xs_port(xs) else {
        return;
    };
    xa.data.set(xs.data());
    xa.datalen.set(xs.datalen().max(0) as usize);
    xa.complete.set(Some(atascsi_atapi_cmd_done));
    xa.timeout.set(xs.timeout.get().max(0) as u32);
    xa.pmp_port.set(ap.ap_pmp_port);
    xa.atascsi_private.set(AtaPrivate::Xs(xs));
    if xs.flags.get() & SCSI_POLL != 0 {
        xa.flags.set(xa.flags.get() | ATA_F_POLL);
    }

    let write = xa.flags.get() & ATA_F_WRITE != 0;
    xa.with_fis(|fis| {
        fis.flags = ap.pmp_flags();
        fis.command = ATA_C_PACKET;
        fis.device = 0;
        fis.sector_count = xa.tag.get() << 3;
        fis.features = ATA_H2D_FEATURES_DMA
            | if write {
                ATA_H2D_FEATURES_DIR_WRITE
            } else {
                ATA_H2D_FEATURES_DIR_READ
            };
        fis.lba_mid = 0x00;
        fis.lba_high = 0x20;
    });

    // Copy SCSI command into ATAPI packet.
    let cmd = xs.cmd.get();
    let len = min(xs.cmdlen.get().max(0) as usize, cmd.as_bytes().len());
    xa.set_packetcmd(&cmd.as_bytes()[..len]);

    ata_exec(as_, xa);
}

/// The sense data of a failed PACKET command, from the ATA error register.
pub fn atascsi_atapi_sense(sd: &mut ScsiSenseData, error: u8) {
    sd.error_code = SSD_ERRCODE_CURRENT;
    sd.flags = (error & 0xf0) >> 4;
    if error & 0x04 != 0 {
        sd.flags = SKEY_ILLEGAL_REQUEST;
    }
    if error & 0x02 != 0 {
        sd.flags |= SSD_EOM;
    }
    if error & 0x01 != 0 {
        sd.flags |= SSD_ILI;
    }
}

/// `atascsi_atapi_cmd_done`.
pub fn atascsi_atapi_cmd_done(xa: &'static AtaXfer) {
    let xs = xa.xs();

    match xa.state() {
        ATA_S_COMPLETE => xs.error.set(XS_NOERROR),
        ATA_S_ERROR => {
            // Return PACKET sense data
            let mut sd = xs.sense.get();
            atascsi_atapi_sense(&mut sd, xa.rfis.get().error);
            xs.sense.set(sd);
            xs.error.set(XS_SENSE);
        }
        ATA_S_TIMEOUT => {
            printf(format_args!("atascsi_atapi_cmd_done, timeout\n"));
            xs.error.set(XS_TIMEOUT);
        }
        s => panic(format_args!(
            "atascsi_atapi_cmd_done: unexpected ata_xfer state ({s})"
        )),
    }

    xs.resid.set(xa.resid.get());

    scsi_done(xs);
}

/// `atascsi_pmp_cmd`: a command for a port multiplier's control port.
pub fn atascsi_pmp_cmd(xs: &'static ScsiXfer) {
    match xs.cmd.get().opcode {
        REQUEST_SENSE => atascsi_pmp_sense(xs),
        INQUIRY => atascsi_pmp_inq(xs),

        TEST_UNIT_READY | PREVENT_ALLOW => atascsi_done(xs, XS_NOERROR),

        _ => atascsi_done(xs, XS_DRIVER_STUFFUP),
    }
}

/// `atascsi_pmp_sense`.
pub fn atascsi_pmp_sense(xs: &'static ScsiXfer) {
    atascsi_no_sense(xs);
}

/// `atascsi_pmp_inq`: a port multiplier's INQUIRY data.
pub fn atascsi_pmp_inq(xs: &'static ScsiXfer) {
    let in_inq: ScsiInquiry = xs.cmd_as();

    if in_inq.flags & SI_EVPD != 0 {
        // any evpd pages we need to support here?
        atascsi_done(xs, XS_DRIVER_STUFFUP);
        return;
    }

    let mut inq = ScsiInquiryData::zeroed();
    inq.device = 0x1E; // "well known logical unit" seems reasonable
    inq.version = SCSI_REV_SPC3;
    inq.response_format = SID_SCSI2_RESPONSE;
    inq.additional_length = SID_SCSI2_ALEN as u8;
    inq.flags |= SID_CmdQue;
    inq.vendor.copy_from_slice(b"ATA     ");

    // should use the data from atascsi_pmp_identify here? not sure how useful the chip id
    // is, but maybe it'd be nice to include the number of ports.
    inq.product.copy_from_slice(b"Port Multiplier\0");
    inq.revision.copy_from_slice(b"    ");

    scsi_copy_internal_data(xs, inq.as_bytes());

    atascsi_done(xs, XS_NOERROR);
}

/// `atascsi_done`.
pub fn atascsi_done(xs: &'static ScsiXfer, error: i32) {
    xs.error.set(error);
    scsi_done(xs);
}

/// `ata_exec`: hands a command to the controller.
pub fn ata_exec(as_: &Atascsi, xa: &'static AtaXfer) {
    (as_.as_methods.ata_cmd)(xa);
}

/// `atascsi_io_get`: the host port iopool's `io_get`: a free command of the port.
///
/// # Safety
///
/// `cookie` is a live [`AtascsiHostPort`] (the one `atascsi_probe` gave
/// `scsi_iopool_init`).
pub unsafe fn atascsi_io_get(cookie: *mut c_void) -> Option<ScsiIo> {
    // SAFETY: the caller's guarantee.
    let ahp = unsafe { &*cookie.cast::<AtascsiHostPort>().cast_const() };
    let as_ = ahp.ahp_as;

    // SAFETY: `as_cookie` is the controller's `aaa_cookie`; `ahp_port` was probed.
    let xa = unsafe { (as_.as_methods.ata_get_xfer)(as_.as_cookie, ahp.ahp_port) }?;
    xa.with_fis(|fis| fis.r#type = ATA_FIS_TYPE_H2D);

    Some(NonNull::from(xa).cast())
}

/// `atascsi_io_put`: the host port iopool's `io_put`.
///
/// # Safety
///
/// `cookie` is a live [`AtascsiHostPort`] and `io` an `ata_xfer` from [`atascsi_io_get`].
pub unsafe fn atascsi_io_put(cookie: *mut c_void, io: ScsiIo) {
    // SAFETY: the caller's guarantee.
    let ahp = unsafe { &*cookie.cast::<AtascsiHostPort>().cast_const() };
    let as_ = ahp.ahp_as;
    // SAFETY: the caller's guarantee: a controller ata_xfer, which lives as long as the
    // controller's port.
    let xa: &'static AtaXfer = unsafe { io.cast::<AtaXfer>().as_ref() };

    xa.set_state(ATA_S_COMPLETE); // XXX this state machine is dumb
    (as_.as_methods.ata_put_xfer)(xa);
}

/// `ata_polled_complete`: the completion of a polled internal command.
pub fn ata_polled_complete(_xa: &'static AtaXfer) {
    // do nothing
}

/// `ata_polled`: the result of a polled internal command, whose xfer goes back to its pool.
pub fn ata_polled(xa: &'static AtaXfer) -> Result<(), Errno> {
    if xa.flags.get() & ATA_F_DONE == 0 {
        panic(format_args!("ata_polled: xa isn't complete"));
    }

    let rv = match xa.state() {
        ATA_S_COMPLETE => Ok(()),
        ATA_S_ERROR | ATA_S_TIMEOUT => Err(Errno::EIO),
        s => panic(format_args!("ata_polled: xa state ({s})")),
    };

    match xa.atascsi_private.get() {
        AtaPrivate::Iopool(iopl) => scsi_io_put(iopl, NonNull::from(xa).cast()),
        _ => panic(format_args!("ata_polled: xa {:p} without an iopool", xa)),
    }

    rv
}

/// `ata_complete`: what the controller calls when a command is over.
pub fn ata_complete(xa: &'static AtaXfer) {
    xa.flags.set(xa.flags.get() | ATA_F_DONE);
    xa.run_complete();
}

/// `ata_swapcopy`: copies `src` into `dst` swapping the bytes of each 16-bit word (ATA
/// strings are big-endian words), `min` of the two lengths rounded down to a word.
pub fn ata_swapcopy(src: &[u8], dst: &mut [u8]) {
    let len = min(src.len(), dst.len()) / 2;

    for i in 0..len {
        dst[2 * i] = src[2 * i + 1];
        dst[2 * i + 1] = src[2 * i];
    }
}

/// `atascsi_port_identify`: IDENTIFY (PACKET) DEVICE into `identify` (512 bytes of DMA-able
/// memory), polled.
pub fn atascsi_port_identify(ap: &'static AtascsiPort, identify: &mut [u8]) -> Result<(), Errno> {
    let as_ = ap.ap_as;
    let ahp = ap.ap_host_port;

    let xa = atascsi_port_xa(ahp);
    xa.pmp_port.set(ap.ap_pmp_port);
    xa.data.set(identify.as_mut_ptr());
    xa.datalen
        .set(min(identify.len(), size_of::<AtaIdentify>()));
    xa.with_fis(|fis| {
        fis.flags = ap.pmp_flags();
        fis.command = if ap.ap_type == ATA_PORT_T_DISK {
            ATA_C_IDENTIFY
        } else {
            ATA_C_IDENTIFY_PACKET
        };
        fis.device = 0;
    });
    xa.flags.set(ATA_F_READ | ATA_F_PIO | ATA_F_POLL);
    xa.timeout.set(1000);
    xa.complete.set(Some(ata_polled_complete));
    xa.atascsi_private.set(AtaPrivate::Iopool(&ahp.ahp_iopool));
    ata_exec(as_, xa);
    ata_polled(xa)
}

/// `atascsi_port_set_features`: SET FEATURES `subcommand` with count `arg`, polled.
pub fn atascsi_port_set_features(
    ap: &'static AtascsiPort,
    subcommand: u8,
    arg: u8,
) -> Result<(), Errno> {
    let as_ = ap.ap_as;
    let ahp = ap.ap_host_port;

    let xa = atascsi_port_xa(ahp);
    xa.with_fis(|fis| {
        fis.command = ATA_C_SET_FEATURES;
        fis.features = subcommand;
        fis.sector_count = arg;
        fis.flags = ap.pmp_flags();
    });
    xa.flags.set(ATA_F_POLL);
    xa.timeout.set(1000);
    xa.complete.set(Some(ata_polled_complete));
    xa.pmp_port.set(ap.ap_pmp_port);
    xa.atascsi_private.set(AtaPrivate::Iopool(&ahp.ahp_iopool));
    ata_exec(as_, xa);
    ata_polled(xa)
}

const _: () = {
    assert!(size_of::<AtaIdentify>() == 512);
    assert!(offset_of!(AtaIdentify, serial) == 10 * 2);
    assert!(offset_of!(AtaIdentify, firmware) == 23 * 2);
    assert!(offset_of!(AtaIdentify, model) == 27 * 2);
    assert!(offset_of!(AtaIdentify, validinfo) == 53 * 2);
    assert!(offset_of!(AtaIdentify, addrsec) == 60 * 2);
    assert!(offset_of!(AtaIdentify, qdepth) == 75 * 2);
    assert!(offset_of!(AtaIdentify, cmdset82) == 82 * 2);
    assert!(offset_of!(AtaIdentify, ultradma) == 88 * 2);
    assert!(offset_of!(AtaIdentify, addrsecxt) == 100 * 2);
    assert!(offset_of!(AtaIdentify, p2l_sect) == 106 * 2);
    assert!(offset_of!(AtaIdentify, naa_ieee_oui) == 108 * 2);
    assert!(offset_of!(AtaIdentify, words_lsec) == 117 * 2);
    assert!(offset_of!(AtaIdentify, rmsn) == 127 * 2);
    assert!(offset_of!(AtaIdentify, form) == 168 * 2);
    assert!(offset_of!(AtaIdentify, curmedser) == 176 * 2);
    assert!(offset_of!(AtaIdentify, logical_align) == 209 * 2);
    assert!(offset_of!(AtaIdentify, integrity) == 255 * 2);
    assert!(size_of::<AtaFisH2d>() == ATA_FIS_LENGTH);
    assert!(size_of::<AtaFisD2h>() == ATA_FIS_LENGTH);
    assert!(size_of::<AtaLogPage10h>() == 512);
};

#[cfg(test)]
mod tests;
