/*	$OpenBSD: biovar.h,v 1.46 2020/06/07 16:51:43 kn Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2002 Niklas Hallqvist.  All rights reserved.
 * Copyright (c) 2005 Marco Peereboom.  All rights reserved.
 * Copyright (c) 2012 Joel Sing <jsing@openbsd.org>.  All rights reserved.
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
 */
/* </LICENSES> */

/* <CODE> */
//! `<dev/biovar.h>`: the bio(4) ioctl interface of RAID controllers and volume managers.
//!
//! Upstream: sys/dev/biovar.h @ 3ce1f3f79392
//!
//! Devices getting ioctls through this interface use ioctl class `'B'` and command numbers
//! starting from 32, lower ones are reserved for generic ioctls. All ioctl data are structures
//! which start with a `struct bio`. `kern_pledge.c`'s `pledge_ioctl` allows `BIOCINQ`,
//! `BIOCDISK`, `BIOCINSTALLBOOT` and friends for the `disklabel` promise.
//!
//! ## Deviations
//! - The `_KERNEL` prototypes (`bio_register`, `bio_unregister`, `bio_status_init`,
//!   `bio_status`, `bio_info`, `bio_warn`, `bio_error`) are functions of `dev/bio.c`; they
//!   belong to its port (`dev/bio.rs`, which now exists), not here.
//! - The anonymous `struct { bdp_percent; bdp_seconds }` that is `bioc_disk`'s `bd_patrol` is
//!   the named [`BiocDiskPatrol`].
//! - `char` arrays are `[u8; N]`, `char *` is `*mut u8` and `void *` is `*mut c_void`: user
//!   addresses, as in C. The structures are `#[repr(C)]` ABI types; their sizes (LP64) and the
//!   offsets after a pointer or a 64-bit field are checked at compile time against what the C
//!   compiler computes.
//! - The `BIOC_*` status values that fill an `int` field are `i32`; the flag bits and the
//!   command masks (`BIOC_INQ` ... `BIOC_DEVLIST`) are `u32`.

use core::ffi::c_void;

use crate::sys::ioccom::_iowr;

/// `BIO_MSG_COUNT`: messages in a `struct bio_status`.
pub const BIO_MSG_COUNT: usize = 5;
/// `BIO_MSG_LEN`: bytes in a `bm_msg`.
pub const BIO_MSG_LEN: usize = 128;

/// `struct bio_msg`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BioMsg {
    /// `bm_type`: one of the `BIO_MSG_*` types.
    pub bm_type: i32,
    /// `bm_msg`: the message, NUL terminated.
    pub bm_msg: [u8; BIO_MSG_LEN],
}

/// `BIO_MSG_INFO`.
pub const BIO_MSG_INFO: i32 = 1;
/// `BIO_MSG_WARN`.
pub const BIO_MSG_WARN: i32 = 2;
/// `BIO_MSG_ERROR`.
pub const BIO_MSG_ERROR: i32 = 3;

/// `struct bio_status`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BioStatus {
    /// `bs_controller`: the controller's device name.
    pub bs_controller: [u8; 16],
    /// `bs_status`: one of the `BIO_STATUS_*` values.
    pub bs_status: i32,
    /// `bs_msg_count`: how many of `bs_msgs` are used.
    pub bs_msg_count: i32,
    /// `bs_msgs`.
    pub bs_msgs: [BioMsg; BIO_MSG_COUNT],
}

/// `BIO_STATUS_UNKNOWN`.
pub const BIO_STATUS_UNKNOWN: i32 = 0;
/// `BIO_STATUS_SUCCESS`.
pub const BIO_STATUS_SUCCESS: i32 = 1;
/// `BIO_STATUS_ERROR`.
pub const BIO_STATUS_ERROR: i32 = 2;

/// `struct bio`: the head of every bio(4) ioctl argument.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Bio {
    /// `bio_cookie`: the device cookie `BIOCLOCATE` returns.
    pub bio_cookie: *mut c_void,
    /// `bio_status`.
    pub bio_status: BioStatus,
}

/// `BIOCLOCATE`: convert name to a cookie.
pub const BIOCLOCATE: u64 = _iowr::<BioLocate>(b'B', 0);
/// `struct bio_locate`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BioLocate {
    /// `bl_bio`.
    pub bl_bio: Bio,
    /// `bl_name`: a user address.
    pub bl_name: *mut u8,
}

/// `BIOCINQ`.
pub const BIOCINQ: u64 = _iowr::<BiocInq>(b'B', 32);
/// `struct bioc_inq`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BiocInq {
    /// `bi_bio`.
    pub bi_bio: Bio,
    /// `bi_dev`: controller device.
    pub bi_dev: [u8; 16],
    /// `bi_novol`: nr of volumes.
    pub bi_novol: i32,
    /// `bi_nodisk`: nr of total disks.
    pub bi_nodisk: i32,
}

/// `BIOCDISK`.
pub const BIOCDISK: u64 = _iowr::<BiocDisk>(b'B', 33);
/// The anonymous `bd_patrol` struct of `struct bioc_disk`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BiocDiskPatrol {
    /// `bdp_percent`.
    pub bdp_percent: i32,
    /// `bdp_seconds`.
    pub bdp_seconds: i32,
}

/// `struct bioc_disk`: a disk in a RAID volume.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BiocDisk {
    /// `bd_bio`.
    pub bd_bio: Bio,

    /// `bd_channel`.
    pub bd_channel: u16,
    /// `bd_target`.
    pub bd_target: u16,
    /// `bd_lun`.
    pub bd_lun: u16,
    /// `bd_other_id`: unused for now.
    pub bd_other_id: u16,

    /// `bd_volid`: associate with volume.
    pub bd_volid: i32,
    /// `bd_diskid`: virtual disk.
    pub bd_diskid: i32,
    /// `bd_status`: current status, one of the `BIOC_SD*` values.
    pub bd_status: i32,
    /// `bd_size`: size of the disk.
    pub bd_size: u64,

    /// `bd_vendor`: scsi string.
    pub bd_vendor: [u8; 32],
    /// `bd_serial`: serial number.
    pub bd_serial: [u8; 32],
    /// `bd_procdev`: processor device.
    pub bd_procdev: [u8; 16],

    /// `bd_patrol`.
    pub bd_patrol: BiocDiskPatrol,
}

/// `BIOC_SDONLINE`.
pub const BIOC_SDONLINE: i32 = 0x00;
/// `BIOC_SDONLINE_S`.
pub const BIOC_SDONLINE_S: &str = "Online";
/// `BIOC_SDOFFLINE`.
pub const BIOC_SDOFFLINE: i32 = 0x01;
/// `BIOC_SDOFFLINE_S`.
pub const BIOC_SDOFFLINE_S: &str = "Offline";
/// `BIOC_SDFAILED`.
pub const BIOC_SDFAILED: i32 = 0x02;
/// `BIOC_SDFAILED_S`.
pub const BIOC_SDFAILED_S: &str = "Failed";
/// `BIOC_SDREBUILD`.
pub const BIOC_SDREBUILD: i32 = 0x03;
/// `BIOC_SDREBUILD_S`.
pub const BIOC_SDREBUILD_S: &str = "Rebuild";
/// `BIOC_SDHOTSPARE`.
pub const BIOC_SDHOTSPARE: i32 = 0x04;
/// `BIOC_SDHOTSPARE_S`.
pub const BIOC_SDHOTSPARE_S: &str = "Hot spare";
/// `BIOC_SDUNUSED`.
pub const BIOC_SDUNUSED: i32 = 0x05;
/// `BIOC_SDUNUSED_S`.
pub const BIOC_SDUNUSED_S: &str = "Unused";
/// `BIOC_SDSCRUB`.
pub const BIOC_SDSCRUB: i32 = 0x06;
/// `BIOC_SDSCRUB_S`.
pub const BIOC_SDSCRUB_S: &str = "Scrubbing";
/// `BIOC_SDINVALID`.
pub const BIOC_SDINVALID: i32 = 0xff;
/// `BIOC_SDINVALID_S`.
pub const BIOC_SDINVALID_S: &str = "Invalid";

/// `BIOCVOL`.
pub const BIOCVOL: u64 = _iowr::<BiocVol>(b'B', 34);
/// `struct bioc_vol`: a RAID volume.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BiocVol {
    /// `bv_bio`.
    pub bv_bio: Bio,
    /// `bv_volid`: volume id.
    pub bv_volid: i32,

    /// `bv_percent`: percent done operation.
    pub bv_percent: i16,
    /// `bv_seconds`: seconds of progress so far.
    pub bv_seconds: u16,

    /// `bv_status`: current status, one of the `BIOC_SV*` values.
    pub bv_status: i32,
    /// `bv_size`: size of the disk.
    pub bv_size: u64,
    /// `bv_level`: raid level.
    pub bv_level: i32,
    /// `bv_nodisk`: nr of drives.
    pub bv_nodisk: i32,
    /// `bv_cache`: cache mode, one of the `BIOC_CV*` values.
    pub bv_cache: i32,

    /// `bv_dev`: device.
    pub bv_dev: [u8; 16],
    /// `bv_vendor`: scsi string.
    pub bv_vendor: [u8; 32],
}

/// `BIOC_SVONLINE`.
pub const BIOC_SVONLINE: i32 = 0x00;
/// `BIOC_SVONLINE_S`.
pub const BIOC_SVONLINE_S: &str = "Online";
/// `BIOC_SVOFFLINE`.
pub const BIOC_SVOFFLINE: i32 = 0x01;
/// `BIOC_SVOFFLINE_S`.
pub const BIOC_SVOFFLINE_S: &str = "Offline";
/// `BIOC_SVDEGRADED`.
pub const BIOC_SVDEGRADED: i32 = 0x02;
/// `BIOC_SVDEGRADED_S`.
pub const BIOC_SVDEGRADED_S: &str = "Degraded";
/// `BIOC_SVBUILDING`.
pub const BIOC_SVBUILDING: i32 = 0x03;
/// `BIOC_SVBUILDING_S`.
pub const BIOC_SVBUILDING_S: &str = "Building";
/// `BIOC_SVSCRUB`.
pub const BIOC_SVSCRUB: i32 = 0x04;
/// `BIOC_SVSCRUB_S`.
pub const BIOC_SVSCRUB_S: &str = "Scrubbing";
/// `BIOC_SVREBUILD`.
pub const BIOC_SVREBUILD: i32 = 0x05;
/// `BIOC_SVREBUILD_S`.
pub const BIOC_SVREBUILD_S: &str = "Rebuild";
/// `BIOC_SVINVALID`.
pub const BIOC_SVINVALID: i32 = 0xff;
/// `BIOC_SVINVALID_S`.
pub const BIOC_SVINVALID_S: &str = "Invalid";

/// `BIOC_CVUNKNOWN`.
pub const BIOC_CVUNKNOWN: i32 = 0x00;
/// `BIOC_CVUNKNOWN_S`.
pub const BIOC_CVUNKNOWN_S: &str = "";
/// `BIOC_CVWRITEBACK`.
pub const BIOC_CVWRITEBACK: i32 = 0x01;
/// `BIOC_CVWRITEBACK_S`.
pub const BIOC_CVWRITEBACK_S: &str = "WB";
/// `BIOC_CVWRITETHROUGH`.
pub const BIOC_CVWRITETHROUGH: i32 = 0x02;
/// `BIOC_CVWRITETHROUGH_S`.
pub const BIOC_CVWRITETHROUGH_S: &str = "WT";

/// `BIOCALARM`.
pub const BIOCALARM: u64 = _iowr::<BiocAlarm>(b'B', 35);
/// `struct bioc_alarm`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BiocAlarm {
    /// `ba_bio`.
    pub ba_bio: Bio,
    /// `ba_opcode`: one of the `BIOC_SA*`/`BIOC_GASTATUS` values.
    pub ba_opcode: i32,

    /// `ba_status`: only used with get state.
    pub ba_status: i32,
}

/// `BIOC_SADISABLE`: disable alarm.
pub const BIOC_SADISABLE: i32 = 0x00;
/// `BIOC_SAENABLE`: enable alarm.
pub const BIOC_SAENABLE: i32 = 0x01;
/// `BIOC_SASILENCE`: silence alarm.
pub const BIOC_SASILENCE: i32 = 0x02;
/// `BIOC_GASTATUS`: get status.
pub const BIOC_GASTATUS: i32 = 0x03;
/// `BIOC_SATEST`: test alarm.
pub const BIOC_SATEST: i32 = 0x04;

/// `BIOCBLINK`.
pub const BIOCBLINK: u64 = _iowr::<BiocBlink>(b'B', 36);
/// `struct bioc_blink`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BiocBlink {
    /// `bb_bio`.
    pub bb_bio: Bio,
    /// `bb_channel`.
    pub bb_channel: u16,
    /// `bb_target`.
    pub bb_target: u16,

    /// `bb_status`: current status, one of the `BIOC_SB*` values.
    pub bb_status: i32,
}

/// `BIOC_SBUNBLINK`: disable blinking.
pub const BIOC_SBUNBLINK: i32 = 0x00;
/// `BIOC_SBBLINK`: enable blink.
pub const BIOC_SBBLINK: i32 = 0x01;
/// `BIOC_SBALARM`: enable alarm blink.
pub const BIOC_SBALARM: i32 = 0x02;

/// `BIOCSETSTATE`.
pub const BIOCSETSTATE: u64 = _iowr::<BiocSetstate>(b'B', 37);
/// `struct bioc_setstate`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BiocSetstate {
    /// `bs_bio`.
    pub bs_bio: Bio,
    /// `bs_channel`.
    pub bs_channel: u16,
    /// `bs_target`.
    pub bs_target: u16,
    /// `bs_lun`.
    pub bs_lun: u16,
    /// `bs_other_id_type`: use other_id instead of ctl; `BIOC_SSOTHER_*`.
    pub bs_other_id_type: u16,
    /// `bs_other_id`: cram dev_t or other id in here.
    pub bs_other_id: i32,

    /// `bs_status`: change to this status, one of the `BIOC_SS*` values.
    pub bs_status: i32,
    /// `bs_volid`: volume id for rebuild.
    pub bs_volid: i32,
}

/// `BIOC_SSOTHER_UNUSED`.
pub const BIOC_SSOTHER_UNUSED: u16 = 0x00;
/// `BIOC_SSOTHER_DEVT`.
pub const BIOC_SSOTHER_DEVT: u16 = 0x01;

/// `BIOC_SSONLINE`: online disk.
pub const BIOC_SSONLINE: i32 = 0x00;
/// `BIOC_SSOFFLINE`: offline disk.
pub const BIOC_SSOFFLINE: i32 = 0x01;
/// `BIOC_SSHOTSPARE`: mark as hotspare.
pub const BIOC_SSHOTSPARE: i32 = 0x02;
/// `BIOC_SSREBUILD`: rebuild on this disk.
pub const BIOC_SSREBUILD: i32 = 0x03;

/// `BIOCCREATERAID`.
pub const BIOCCREATERAID: u64 = _iowr::<BiocCreateraid>(b'B', 38);
/// `struct bioc_createraid`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BiocCreateraid {
    /// `bc_bio`.
    pub bc_bio: Bio,
    /// `bc_dev_list`: a user address.
    pub bc_dev_list: *mut c_void,
    /// `bc_dev_list_len`.
    pub bc_dev_list_len: u16,
    /// `bc_key_disk`.
    pub bc_key_disk: i32,
    /// `bc_level`.
    pub bc_level: u16,
    /// `bc_flags`: `BIOC_SC*` bits.
    pub bc_flags: u32,
    /// `bc_opaque_size`.
    pub bc_opaque_size: u32,
    /// `bc_opaque_flags`: one of the `BIOC_SO*` values.
    pub bc_opaque_flags: u32,
    /// `bc_opaque_status`: `BIOC_SOINOUT_*`.
    pub bc_opaque_status: u32,
    /// `bc_opaque`: a user address.
    pub bc_opaque: *mut c_void,
}

/// `BIOC_CRMAXLEN`.
pub const BIOC_CRMAXLEN: i32 = 1024;

/// `BIOC_SCFORCE`: do not assemble, force create.
pub const BIOC_SCFORCE: u32 = 0x01;
/// `BIOC_SCDEVT`: dev_t array or string in dev_list.
pub const BIOC_SCDEVT: u32 = 0x02;
/// `BIOC_SCNOAUTOASSEMBLE`: do not assemble during autoconf.
pub const BIOC_SCNOAUTOASSEMBLE: u32 = 0x04;
/// `BIOC_SCBOOTABLE`: device is bootable.
pub const BIOC_SCBOOTABLE: u32 = 0x08;

/// `BIOC_SOINVALID`: no opaque pointer.
pub const BIOC_SOINVALID: u32 = 0x00;
/// `BIOC_SOIN`: kernel perspective direction.
pub const BIOC_SOIN: u32 = 0x01;
/// `BIOC_SOOUT`: kernel perspective direction.
pub const BIOC_SOOUT: u32 = 0x02;

/// `BIOC_SOINOUT_FAILED`: operation failed.
pub const BIOC_SOINOUT_FAILED: u32 = 0x00;
/// `BIOC_SOINOUT_OK`: operation succeeded.
pub const BIOC_SOINOUT_OK: u32 = 0x01;

/// `BIOCDELETERAID`.
pub const BIOCDELETERAID: u64 = _iowr::<BiocDeleteraid>(b'B', 39);
/// `struct bioc_deleteraid`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BiocDeleteraid {
    /// `bd_bio`.
    pub bd_bio: Bio,
    /// `bd_flags`: `BIOC_SDCLEARMETA`.
    pub bd_flags: u32,
    /// `bd_dev`: device.
    pub bd_dev: [u8; 16],
}

/// `BIOC_SDCLEARMETA`: clear metadata region.
pub const BIOC_SDCLEARMETA: u32 = 0x01;

/// `BIOCDISCIPLINE`.
pub const BIOCDISCIPLINE: u64 = _iowr::<BiocDiscipline>(b'B', 40);
/// `struct bioc_discipline`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BiocDiscipline {
    /// `bd_bio`.
    pub bd_bio: Bio,
    /// `bd_dev`.
    pub bd_dev: [u8; 16],
    /// `bd_cmd`.
    pub bd_cmd: u32,
    /// `bd_size`.
    pub bd_size: u32,
    /// `bd_data`: a user address.
    pub bd_data: *mut c_void,
}

/// `BIOCINSTALLBOOT`.
pub const BIOCINSTALLBOOT: u64 = _iowr::<BiocInstallboot>(b'B', 41);
/// `struct bioc_installboot`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BiocInstallboot {
    /// `bb_bio`.
    pub bb_bio: Bio,
    /// `bb_dev`.
    pub bb_dev: [u8; 16],
    /// `bb_bootblk`: a user address.
    pub bb_bootblk: *mut c_void,
    /// `bb_bootldr`: a user address.
    pub bb_bootldr: *mut c_void,
    /// `bb_bootblk_size`.
    pub bb_bootblk_size: u32,
    /// `bb_bootldr_size`.
    pub bb_bootldr_size: u32,
}

/// `BIOCPATROL`.
pub const BIOCPATROL: u64 = _iowr::<BiocPatrol>(b'B', 42);
/// `struct bioc_patrol`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BiocPatrol {
    /// `bp_bio`.
    pub bp_bio: Bio,
    /// `bp_opcode`: one of the `BIOC_SP*`/`BIOC_GPSTATUS` values.
    pub bp_opcode: i32,

    /// `bp_mode`: one of the `BIOC_SPM*` values.
    pub bp_mode: i32,
    /// `bp_status`: only used with get state; `BIOC_SPS*`.
    pub bp_status: i32,

    /// `bp_autoival`.
    pub bp_autoival: i32,
    /// `bp_autonext`.
    pub bp_autonext: i32,
    /// `bp_autonow`.
    pub bp_autonow: i32,
}

/// `BIOC_SPSTOP`: stop patrol.
pub const BIOC_SPSTOP: i32 = 0x00;
/// `BIOC_SPSTART`: start patrol.
pub const BIOC_SPSTART: i32 = 0x01;
/// `BIOC_GPSTATUS`: get status.
pub const BIOC_GPSTATUS: i32 = 0x02;
/// `BIOC_SPDISABLE`: disable patrol.
pub const BIOC_SPDISABLE: i32 = 0x03;
/// `BIOC_SPAUTO`: enable patrol as auto.
pub const BIOC_SPAUTO: i32 = 0x04;
/// `BIOC_SPMANUAL`: enable patrol as manual.
pub const BIOC_SPMANUAL: i32 = 0x05;

/// `BIOC_SPMAUTO`.
pub const BIOC_SPMAUTO: i32 = 0x00;
/// `BIOC_SPMMANUAL`.
pub const BIOC_SPMMANUAL: i32 = 0x01;
/// `BIOC_SPMDISABLED`.
pub const BIOC_SPMDISABLED: i32 = 0x02;

/// `BIOC_SPSSTOPPED`.
pub const BIOC_SPSSTOPPED: i32 = 0x00;
/// `BIOC_SPSREADY`.
pub const BIOC_SPSREADY: i32 = 0x01;
/// `BIOC_SPSACTIVE`.
pub const BIOC_SPSACTIVE: i32 = 0x02;
/// `BIOC_SPSABORTED`.
pub const BIOC_SPSABORTED: i32 = 0xff;

// kernel and userspace defines

/// `BIOC_INQ`.
pub const BIOC_INQ: u32 = 0x0001;
/// `BIOC_DISK`.
pub const BIOC_DISK: u32 = 0x0002;
/// `BIOC_VOL`.
pub const BIOC_VOL: u32 = 0x0004;
/// `BIOC_ALARM`.
pub const BIOC_ALARM: u32 = 0x0008;
/// `BIOC_BLINK`.
pub const BIOC_BLINK: u32 = 0x0010;
/// `BIOC_SETSTATE`.
pub const BIOC_SETSTATE: u32 = 0x0020;
/// `BIOC_CREATERAID`.
pub const BIOC_CREATERAID: u32 = 0x0040;
/// `BIOC_DELETERAID`.
pub const BIOC_DELETERAID: u32 = 0x0080;
/// `BIOC_DISCIPLINE`.
pub const BIOC_DISCIPLINE: u32 = 0x0100;
/// `BIOC_INSTALLBOOT`.
pub const BIOC_INSTALLBOOT: u32 = 0x0200;
/// `BIOC_PATROL`.
pub const BIOC_PATROL: u32 = 0x0400;

// user space defines

/// `BIOC_DEVLIST`.
pub const BIOC_DEVLIST: u32 = 0x10000;

// Sizes (LP64: amd64 and arm64) and the offsets after pointers and 64-bit fields, as the C
// compiler lays the structures out.
const _: () = {
    assert!(size_of::<BioMsg>() == 132);
    assert!(size_of::<BioStatus>() == 684);
    assert!(size_of::<Bio>() == 696);
    assert!(size_of::<BioLocate>() == 704);
    assert!(size_of::<BiocInq>() == 720);
    assert!(size_of::<BiocDisk>() == 816);
    assert!(size_of::<BiocVol>() == 784);
    assert!(size_of::<BiocAlarm>() == 704);
    assert!(size_of::<BiocBlink>() == 704);
    assert!(size_of::<BiocSetstate>() == 720);
    assert!(size_of::<BiocCreateraid>() == 744);
    assert!(size_of::<BiocDeleteraid>() == 720);
    assert!(size_of::<BiocDiscipline>() == 728);
    assert!(size_of::<BiocInstallboot>() == 736);
    assert!(size_of::<BiocPatrol>() == 720);
    assert!(core::mem::offset_of!(BiocDisk, bd_size) == 720);
    assert!(core::mem::offset_of!(BiocDisk, bd_vendor) == 728);
    assert!(core::mem::offset_of!(BiocDisk, bd_patrol) == 808);
    assert!(core::mem::offset_of!(BiocVol, bv_size) == 712);
    assert!(core::mem::offset_of!(BiocCreateraid, bc_dev_list) == 696);
    assert!(core::mem::offset_of!(BiocCreateraid, bc_opaque) == 736);
    assert!(core::mem::offset_of!(BiocInstallboot, bb_bootblk) == 712);
    assert!(core::mem::offset_of!(BiocDiscipline, bd_data) == 720);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_have_openbsd_values() {
        // The values the C compiler computes for _IOWR('B', n, struct ...) on LP64.
        assert_eq!(BIOCLOCATE, 0xc2c0_4200);
        assert_eq!(BIOCINQ, 0xc2d0_4220);
        assert_eq!(BIOCDISK, 0xc330_4221);
        assert_eq!(BIOCVOL, 0xc310_4222);
        assert_eq!(BIOCALARM, 0xc2c0_4223);
        assert_eq!(BIOCBLINK, 0xc2c0_4224);
        assert_eq!(BIOCSETSTATE, 0xc2d0_4225);
        assert_eq!(BIOCCREATERAID, 0xc2e8_4226);
        assert_eq!(BIOCDELETERAID, 0xc2d0_4227);
        assert_eq!(BIOCDISCIPLINE, 0xc2d8_4228);
        assert_eq!(BIOCINSTALLBOOT, 0xc2e0_4229);
        assert_eq!(BIOCPATROL, 0xc2d0_422a);
    }

    /// Every `BIOC_*_S` and the like: a string constant equals the C string literal.
    macro_rules! assert_strings {
    ($defs:expr; $($name:ident),* $(,)?) => {{
        let mut names: std::vec::Vec<&'static str> = std::vec::Vec::new();
        $(
            names.push(stringify!($name));
            assert_eq!(
                $defs.get(stringify!($name)).map(|s| s.as_str()),
                Some(std::format!("{:?}", $name).as_str()),
                "{}",
                stringify!($name)
            );
        )*
        names
    }};
}

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/biovar.h");
        let mut ours = crate::reftest::assert_defines!(defs;
        BIO_MSG_COUNT, BIO_MSG_LEN, BIO_MSG_INFO, BIO_MSG_WARN, BIO_MSG_ERROR,
        BIO_STATUS_UNKNOWN, BIO_STATUS_SUCCESS, BIO_STATUS_ERROR,
        BIOC_SDONLINE, BIOC_SDOFFLINE, BIOC_SDFAILED, BIOC_SDREBUILD, BIOC_SDHOTSPARE,
        BIOC_SDUNUSED, BIOC_SDSCRUB, BIOC_SDINVALID,
        BIOC_SVONLINE, BIOC_SVOFFLINE, BIOC_SVDEGRADED, BIOC_SVBUILDING, BIOC_SVSCRUB,
        BIOC_SVREBUILD, BIOC_SVINVALID,
        BIOC_CVUNKNOWN, BIOC_CVWRITEBACK, BIOC_CVWRITETHROUGH,
        BIOC_SADISABLE, BIOC_SAENABLE, BIOC_SASILENCE, BIOC_GASTATUS, BIOC_SATEST,
        BIOC_SBUNBLINK, BIOC_SBBLINK, BIOC_SBALARM,
        BIOC_SSOTHER_UNUSED, BIOC_SSOTHER_DEVT,
        BIOC_SSONLINE, BIOC_SSOFFLINE, BIOC_SSHOTSPARE, BIOC_SSREBUILD);
        ours.extend(crate::reftest::assert_defines!(defs;
        BIOC_CRMAXLEN, BIOC_SCFORCE, BIOC_SCDEVT, BIOC_SCNOAUTOASSEMBLE, BIOC_SCBOOTABLE,
        BIOC_SOINVALID, BIOC_SOIN, BIOC_SOOUT, BIOC_SOINOUT_FAILED, BIOC_SOINOUT_OK,
        BIOC_SDCLEARMETA,
        BIOC_SPSTOP, BIOC_SPSTART, BIOC_GPSTATUS, BIOC_SPDISABLE, BIOC_SPAUTO, BIOC_SPMANUAL,
        BIOC_SPMAUTO, BIOC_SPMMANUAL, BIOC_SPMDISABLED,
        BIOC_SPSSTOPPED, BIOC_SPSREADY, BIOC_SPSACTIVE, BIOC_SPSABORTED));
        ours.extend(crate::reftest::assert_defines!(defs;
        BIOC_INQ, BIOC_DISK, BIOC_VOL, BIOC_ALARM, BIOC_BLINK, BIOC_SETSTATE,
        BIOC_CREATERAID, BIOC_DELETERAID, BIOC_DISCIPLINE, BIOC_INSTALLBOOT, BIOC_PATROL,
        BIOC_DEVLIST));
        ours.extend(assert_strings!(defs;
        BIOC_SDONLINE_S, BIOC_SDOFFLINE_S, BIOC_SDFAILED_S, BIOC_SDREBUILD_S,
        BIOC_SDHOTSPARE_S, BIOC_SDUNUSED_S, BIOC_SDSCRUB_S, BIOC_SDINVALID_S,
        BIOC_SVONLINE_S, BIOC_SVOFFLINE_S, BIOC_SVDEGRADED_S, BIOC_SVBUILDING_S,
        BIOC_SVSCRUB_S, BIOC_SVREBUILD_S, BIOC_SVINVALID_S,
        BIOC_CVUNKNOWN_S, BIOC_CVWRITEBACK_S, BIOC_CVWRITETHROUGH_S));
        // The ioctl numbers are _IOWR expressions, which the helper does not evaluate:
        // `commands_have_openbsd_values` pins their values, this pins the expressions.
        let ioctls = [
            ("BIOCLOCATE", "_IOWR('B', 0, struct bio_locate)"),
            ("BIOCINQ", "_IOWR('B', 32, struct bioc_inq)"),
            ("BIOCDISK", "_IOWR('B', 33, struct bioc_disk)"),
            ("BIOCVOL", "_IOWR('B', 34, struct bioc_vol)"),
            ("BIOCALARM", "_IOWR('B', 35, struct bioc_alarm)"),
            ("BIOCBLINK", "_IOWR('B', 36, struct bioc_blink)"),
            ("BIOCSETSTATE", "_IOWR('B', 37, struct bioc_setstate)"),
            ("BIOCCREATERAID", "_IOWR('B', 38, struct bioc_createraid)"),
            ("BIOCDELETERAID", "_IOWR('B', 39, struct bioc_deleteraid)"),
            ("BIOCDISCIPLINE", "_IOWR('B', 40, struct bioc_discipline)"),
            ("BIOCINSTALLBOOT", "_IOWR('B', 41, struct bioc_installboot)"),
            ("BIOCPATROL", "_IOWR('B', 42, struct bioc_patrol)"),
        ];
        for (name, text) in ioctls {
            assert_eq!(defs.get(name).map(|s| s.as_str()), Some(text), "{name}");
            ours.push(name);
        }
        crate::reftest::assert_complete(&defs, "BIO", &ours);
    }
}
/* </TESTS> */
