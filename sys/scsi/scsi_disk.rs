/*	$OpenBSD: scsi_disk.h,v 1.43 2022/01/11 23:10:11 jsg Exp $	*/
/*	$NetBSD: scsi_disk.h,v 1.10 1996/07/05 16:19:05 christos Exp $	*/
/* <LICENSES> */
/*
 * SCSI interface description
 */

/*
 * Some lines of this file come from a file of the name "scsi.h"
 * distributed by OSF as part of mach2.5,
 *  so the following disclaimer has been kept.
 *
 * Copyright 1990 by Open Software Foundation,
 * Grenoble, FRANCE
 *
 *		All Rights Reserved
 *
 *   Permission to use, copy, modify, and distribute this software and
 * its documentation for any purpose and without fee is hereby granted,
 * provided that the above copyright notice appears in all copies and
 * that both the copyright notice and this permission notice appear in
 * supporting documentation, and that the name of OSF or Open Software
 * Foundation not be used in advertising or publicity pertaining to
 * distribution of the software without specific, written prior
 * permission.
 *
 *   OSF DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE
 * INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS,
 * IN NO EVENT SHALL OSF BE LIABLE FOR ANY SPECIAL, INDIRECT, OR
 * CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
 * LOSS OF USE, DATA OR PROFITS, WHETHER IN ACTION OF CONTRACT,
 * NEGLIGENCE, OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION
 * WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*
 * Largely written by Julian Elischer (julian@tfs.com)
 * for TRW Financial Systems.
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

//! `<scsi/scsi_disk.h>`: the SCSI commands, mode pages and vital product data pages of direct
//! access (disk) devices: FORMAT UNIT, REASSIGN BLOCKS, REZERO UNIT, READ and WRITE (6, 10, 12
//! and 16 byte), WRITE SAME, UNMAP, READ CAPACITY (10 and 16), START STOP UNIT and SYNCHRONIZE
//! CACHE, the disk mode pages (format, rigid geometry, flexible geometry, reduced geometry,
//! caching) and the block limits, device characteristics and thin provisioning VPD pages.
//!
//! Upstream: sys/scsi/scsi_disk.h @ 3ce1f3f79392
//!
//! Every structure here is a wire format in the sense of [`crate::scsi::scsi_all`]:
//! `#[repr(C)]` made of `u8`s and byte arrays only, implementing [`ScsiWire`], with its size
//! pinned at the end of the file. Structure names are the C names in CamelCase
//! (`struct scsi_rw_10` is [`ScsiRw10`], `struct page_disk_format` is [`PageDiskFormat`]);
//! constants keep their C names.
//!
//! ## Deviations
//! - The anonymous structure inside `struct scsi_reassign_blocks_data` (`defect_descriptor[1]`,
//!   one `dlbaddr[4]`) is [`ScsiReassignBlocksDefectDescriptor`].
//! - `PG_CACHE_PRI_DEMAND(_f)` and `PG_CACHE_PRI_WRITE(_f)` are the `const fn`s
//!   [`pg_cache_pri_demand`] and [`pg_cache_pri_write`].
//! - The `#if 0` pattern member of `struct scsi_initialization_pattern_descriptor` is not
//!   ported: it is dead code in the C as well.
//! - `SI_PG_DISK_LIMITS_UGAVALID` is `1U << 31`, hence a `u32`; the other flags are `u8`s,
//!   except `VPD_DISK_INFO_RPM_*` (a `u16`, compared with a 2-byte field).

use core::mem::size_of;

use crate::scsi::scsi_all::{ScsiVpdHdr, ScsiWire};

/// Marks wire structures (see [`ScsiWire`]).
macro_rules! scsi_wire {
    ($($t:ty),* $(,)?) => {
        $(
            // SAFETY: `#[repr(C)]` of `u8`s, byte arrays and structures of them only (the size
            // assertions at the end of the file pin that there is no padding).
            unsafe impl ScsiWire for $t {}
        )*
    };
}

/// `FORMAT_UNIT`.
pub const FORMAT_UNIT: u8 = 0x04;
/// `struct scsi_format_unit`: FORMAT UNIT.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiFormatUnit {
    /// `opcode`.
    pub opcode: u8,
    /// `flags`: `SFU_*`.
    pub flags: u8,
    /// `vendor_specific`.
    pub vendor_specific: u8,
    /// `interleave`.
    pub interleave: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `SFU_DLF_MASK`: `scsi_format_unit.flags`.
pub const SFU_DLF_MASK: u8 = 0x07;
/// `SFU_CMPLST`.
pub const SFU_CMPLST: u8 = 0x08;
/// `SFU_FMTDATA`.
pub const SFU_FMTDATA: u8 = 0x10;

/*
 * If the FmtData bit is set, a FORMAT UNIT parameter list is transferred
 * to the target during the DATA OUT phase.  The parameter list includes
 *
 *	Defect list header
 *	Initialization pattern descriptor (if any)
 *	Defect descriptor(s) (if any)
 */
/// `struct scsi_format_unit_defect_list_header`: the defect list header of a FORMAT UNIT parameter list.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiFormatUnitDefectListHeader {
    /// `reserved`.
    pub reserved: u8,
    /// `flags`: `DLH_*`.
    pub flags: u8,
    /// `defect_lst_len`.
    pub defect_lst_len: [u8; 2],
}

/// `DLH_VS`: vendor specific.
pub const DLH_VS: u8 = 0x01;
/// `DLH_IMMED`: immediate return.
pub const DLH_IMMED: u8 = 0x02;
/// `DLH_DSP`: disable saving parameters.
pub const DLH_DSP: u8 = 0x04;
/// `DLH_IP`: initialization pattern.
pub const DLH_IP: u8 = 0x08;
/// `DLH_STPF`: stop format.
pub const DLH_STPF: u8 = 0x10;
/// `DLH_DCRT`: disable certification.
pub const DLH_DCRT: u8 = 0x20;
/// `DLH_DPRY`: disable primary.
pub const DLH_DPRY: u8 = 0x40;
/// `DLH_FOV`: format options valid.
pub const DLH_FOV: u8 = 0x80;

/*
 * See Table 117 of the SCSI-2 specification for a description of
 * the IP modifier.
 */
/// `struct scsi_initialization_pattern_descriptor`: the initialization pattern descriptor of a FORMAT UNIT parameter list (the pattern itself follows).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiInitializationPatternDescriptor {
    /// `ip_modifier`.
    pub ip_modifier: u8,
    /// `pattern_type`: `IP_TYPE_*`.
    pub pattern_type: u8,
    /// `pattern_length`.
    pub pattern_length: [u8; 2],
}

/// `IP_TYPE_DEFAULT`: `pattern_type`; 0x02 to 0x7f are reserved, 0x80 to 0xff vendor-specific.
pub const IP_TYPE_DEFAULT: u8 = 0x01;
/// `IP_TYPE_REPEAT`.
pub const IP_TYPE_REPEAT: u8 = 0x01;

/*
 * Defect descriptors.  These are used as the defect lists in the FORMAT UNIT
 * and READ DEFECT DATA commands, and as the translate page of the
 * SEND DIAGNOSTIC and RECEIVE DIAGNOSTIC RESULTS commands.
 */
/// `struct scsi_defect_descriptor_bf`: a defect descriptor, block format.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiDefectDescriptorBf {
    /// `block_address`.
    pub block_address: [u8; 4],
}

/// `struct scsi_defect_descriptor_bfif`: a defect descriptor, bytes from index format.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiDefectDescriptorBfif {
    /// `cylinder`.
    pub cylinder: [u8; 2],
    /// `head`.
    pub head: u8,
    /// `bytes_from_index`.
    pub bytes_from_index: [u8; 2],
}

/// `struct scsi_defect_descriptor_psf`: a defect descriptor, physical sector format.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiDefectDescriptorPsf {
    /// `cylinder`.
    pub cylinder: [u8; 2],
    /// `head`.
    pub head: u8,
    /// `sector`.
    pub sector: [u8; 2],
}

/// `struct scsi_reassign_blocks`: REASSIGN BLOCKS.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReassignBlocks {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `unused`.
    pub unused: [u8; 3],
    /// `control`.
    pub control: u8,
}

/// `REZERO_UNIT`.
pub const REZERO_UNIT: u8 = 0x01;
/// `struct scsi_rezero_unit`: REZERO UNIT.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiRezeroUnit {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `reserved`.
    pub reserved: [u8; 3],
    /// `control`.
    pub control: u8,
}

/// `struct scsi_rw`: READ (6) and WRITE (6): `READ_COMMAND` and `WRITE_COMMAND`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiRw {
    /// `opcode`.
    pub opcode: u8,
    /// `addr`: the top byte holds only 5 bits, `SRW_TOPADDR`.
    pub addr: [u8; 3],
    /// `length`.
    pub length: u8,
    /// `control`.
    pub control: u8,
}

/// `SRW_TOPADDR`: `scsi_rw.addr[0]`: only 5 bits here.
pub const SRW_TOPADDR: u8 = 0x1F;
/// `struct scsi_rw_10`: READ (10) and WRITE (10).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiRw10 {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`: `SRWB_RELADDR`.
    pub byte2: u8,
    /// `addr`.
    pub addr: [u8; 4],
    /// `reserved`.
    pub reserved: u8,
    /// `length`.
    pub length: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `SRWB_RELADDR`: `scsi_rw_10.byte2`.
pub const SRWB_RELADDR: u8 = 0x01;
/// `struct scsi_rw_12`: READ (12) and WRITE (12).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiRw12 {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `addr`.
    pub addr: [u8; 4],
    /// `length`.
    pub length: [u8; 4],
    /// `reserved`.
    pub reserved: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_rw_16`: READ (16) and WRITE (16).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiRw16 {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `addr`.
    pub addr: [u8; 8],
    /// `length`.
    pub length: [u8; 4],
    /// `reserved`.
    pub reserved: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_write_same_10`: WRITE SAME (10).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiWriteSame10 {
    /// `opcode`.
    pub opcode: u8,
    /// `flags`: `WRITE_SAME_F_*`.
    pub flags: u8,
    /// `lba`.
    pub lba: [u8; 4],
    /// `group_number`.
    pub group_number: u8,
    /// `length`.
    pub length: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `WRITE_SAME_F_LBDATA`: `scsi_write_same_10.flags`.
pub const WRITE_SAME_F_LBDATA: u8 = 1 << 1;
/// `WRITE_SAME_F_PBDATA`.
pub const WRITE_SAME_F_PBDATA: u8 = 1 << 2;
/// `struct scsi_write_same_16`: WRITE SAME (16).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiWriteSame16 {
    /// `opcode`.
    pub opcode: u8,
    /// `flags`: includes the WRITE SAME (10) flags.
    pub flags: u8,
    /// `lba`.
    pub lba: [u8; 8],
    /// `length`.
    pub length: [u8; 4],
    /// `group_number`.
    pub group_number: u8,
    /// `control`.
    pub control: u8,
}

/// `WRITE_SAME_F_UNMAP`: `scsi_write_same_16.flags`, in addition to the WRITE SAME (10) flags.
pub const WRITE_SAME_F_UNMAP: u8 = 1 << 3;
/// `WRITE_SAME_F_ANCHOR`.
pub const WRITE_SAME_F_ANCHOR: u8 = 1 << 4;
/// `struct scsi_unmap`: UNMAP.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiUnmap {
    /// `opcode`.
    pub opcode: u8,
    /// `anchor`.
    pub anchor: u8,
    /// `_reserved`.
    pub _reserved: [u8; 4],
    /// `group_number`.
    pub group_number: u8,
    /// `list_len`.
    pub list_len: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `struct scsi_unmap_data`: the UNMAP parameter list header, followed by [`ScsiUnmapDesc`]s.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiUnmapData {
    /// `data_length`.
    pub data_length: [u8; 2],
    /// `desc_length`.
    pub desc_length: [u8; 2],
    /// `_reserved`.
    pub _reserved: [u8; 4],
}

/// `struct scsi_unmap_desc`: an UNMAP block descriptor.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiUnmapDesc {
    /// `logical_addr`.
    pub logical_addr: [u8; 8],
    /// `logical_blocks`.
    pub logical_blocks: [u8; 4],
    /// `_reserved`.
    pub _reserved: [u8; 4],
}

/// `struct scsi_read_capacity`: READ CAPACITY (10).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReadCapacity {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `addr`.
    pub addr: [u8; 4],
    /// `unused`.
    pub unused: [u8; 3],
    /// `control`.
    pub control: u8,
}

/// `struct scsi_read_capacity_16`: READ CAPACITY (16), a service action of `READ_CAPACITY_16`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReadCapacity16 {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`: `SRC16_SERVICE_ACTION`.
    pub byte2: u8,
    /// `addr`.
    pub addr: [u8; 8],
    /// `length`.
    pub length: [u8; 4],
    /// `reserved`.
    pub reserved: u8,
    /// `control`.
    pub control: u8,
}

/// `SRC16_SERVICE_ACTION`: `scsi_read_capacity_16.byte2`.
pub const SRC16_SERVICE_ACTION: u8 = 0x10;
/// `struct scsi_start_stop`: START STOP UNIT.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiStartStop {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `unused`.
    pub unused: [u8; 2],
    /// `how`: `SSS_*`.
    pub how: u8,
    /// `control`.
    pub control: u8,
}

/// `SSS_STOP`: `scsi_start_stop.how`.
pub const SSS_STOP: u8 = 0x00;
/// `SSS_START`.
pub const SSS_START: u8 = 0x01;
/// `SSS_LOEJ`.
pub const SSS_LOEJ: u8 = 0x02;

/*
 * XXX Does ATAPI have an equivalent?
 */
/// `struct scsi_synchronize_cache`: SYNCHRONIZE CACHE (10).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiSynchronizeCache {
    /// `opcode`.
    pub opcode: u8,
    /// `flags`: `SSC_*`.
    pub flags: u8,
    /// `addr`.
    pub addr: [u8; 4],
    /// `reserved`.
    pub reserved: u8,
    /// `length`.
    pub length: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `SSC_RELADR`: `scsi_synchronize_cache.flags`.
pub const SSC_RELADR: u8 = 0x01;
/// `SSC_IMMED`.
pub const SSC_IMMED: u8 = 0x02;

/*
 * Disk specific opcodes
 */
/// `REASSIGN_BLOCKS`.
pub const REASSIGN_BLOCKS: u8 = 0x07;
/// `READ_COMMAND`.
pub const READ_COMMAND: u8 = 0x08;
/// `WRITE_COMMAND`.
pub const WRITE_COMMAND: u8 = 0x0a;
/// `READ_CAPACITY`.
pub const READ_CAPACITY: u8 = 0x25;
/// `READ_CAPACITY_16`.
pub const READ_CAPACITY_16: u8 = 0x9e;
/// `READ_10`.
pub const READ_10: u8 = 0x28;
/// `WRITE_10`.
pub const WRITE_10: u8 = 0x2a;
/// `READ_12`.
pub const READ_12: u8 = 0xa8;
/// `WRITE_12`.
pub const WRITE_12: u8 = 0xaa;
/// `READ_16`.
pub const READ_16: u8 = 0x88;
/// `WRITE_16`.
pub const WRITE_16: u8 = 0x8a;
/// `SYNCHRONIZE_CACHE`.
pub const SYNCHRONIZE_CACHE: u8 = 0x35;
/// `WRITE_SAME_10`.
pub const WRITE_SAME_10: u8 = 0x41;
/// `WRITE_SAME_16`.
pub const WRITE_SAME_16: u8 = 0x93;
/// `UNMAP`.
pub const UNMAP: u8 = 0x42;
/// `struct scsi_reassign_blocks_data::defect_descriptor`: one defect descriptor of [`ScsiReassignBlocksData`] (an anonymous structure in the C).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReassignBlocksDefectDescriptor {
    /// `dlbaddr`.
    pub dlbaddr: [u8; 4],
}

/// `struct scsi_reassign_blocks_data`: the REASSIGN BLOCKS parameter list.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReassignBlocksData {
    /// `reserved`.
    pub reserved: [u8; 2],
    /// `length`.
    pub length: [u8; 2],
    /// `defect_descriptor`.
    pub defect_descriptor: [ScsiReassignBlocksDefectDescriptor; 1],
}

/* Only the lower 6 bits of the pg_code field are used for page #. */
/// `PAGE_DISK_FORMAT`.
pub const PAGE_DISK_FORMAT: u8 = 3;
/// `PAGE_RIGID_GEOMETRY`.
pub const PAGE_RIGID_GEOMETRY: u8 = 4;
/// `PAGE_FLEX_GEOMETRY`.
pub const PAGE_FLEX_GEOMETRY: u8 = 5;
/// `PAGE_REDUCED_GEOMETRY`.
pub const PAGE_REDUCED_GEOMETRY: u8 = 6;
/// `PAGE_CACHING_MODE`.
pub const PAGE_CACHING_MODE: u8 = 8;
/// `struct page_disk_format`: the format device mode page.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PageDiskFormat {
    /// `pg_code`: page code (should be 3).
    pub pg_code: u8,
    /// `pg_length`: page length (should be 0x16).
    pub pg_length: u8,
    /// `trk_z`: tracks per zone.
    pub trk_z: [u8; 2],
    /// `alt_sec`: alternate sectors per zone.
    pub alt_sec: [u8; 2],
    /// `alt_trk_z`: alternate tracks per zone.
    pub alt_trk_z: [u8; 2],
    /// `alt_trk_v`: alternate tracks per volume.
    pub alt_trk_v: [u8; 2],
    /// `ph_sec_t`: physical sectors per track.
    pub ph_sec_t: [u8; 2],
    /// `bytes_s`: bytes per sector.
    pub bytes_s: [u8; 2],
    /// `interleave`: interleave.
    pub interleave: [u8; 2],
    /// `trk_skew`: track skew factor.
    pub trk_skew: [u8; 2],
    /// `cyl_skew`: cylinder skew.
    pub cyl_skew: [u8; 2],
    /// `flags`: various: `DISK_FMT_*`.
    pub flags: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `reserved2`.
    pub reserved2: u8,
    /// `reserved3`.
    pub reserved3: u8,
}

/// `DISK_FMT_SURF`: `page_disk_format.flags`.
pub const DISK_FMT_SURF: u8 = 0x10;
/// `DISK_FMT_RMB`.
pub const DISK_FMT_RMB: u8 = 0x20;
/// `DISK_FMT_HSEC`.
pub const DISK_FMT_HSEC: u8 = 0x40;
/// `DISK_FMT_SSEC`.
pub const DISK_FMT_SSEC: u8 = 0x80;
/// `struct page_rigid_geometry`: the rigid disk geometry mode page.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PageRigidGeometry {
    /// `pg_code`: page code (should be 4).
    pub pg_code: u8,
    /// `pg_length`: page length (should be 0x12 or 0x16).
    pub pg_length: u8,
    /// `ncyl`: number of cylinders.
    pub ncyl: [u8; 3],
    /// `nheads`: number of heads.
    pub nheads: u8,
    /// `st_cyl_wp`: starting cyl., write precomp.
    pub st_cyl_wp: [u8; 3],
    /// `st_cyl_rwc`: starting cyl., red. write cur.
    pub st_cyl_rwc: [u8; 3],
    /// `driv_step`: drive step rate.
    pub driv_step: [u8; 2],
    /// `land_zone`: landing zone cylinder.
    pub land_zone: [u8; 3],
    /// `sp_sync_ctl`: spindle synch control: `SPINDLE_SYNCH_*`.
    pub sp_sync_ctl: u8,
    /// `rot_offset`: rotational offset (for spindle synch).
    pub rot_offset: u8,
    /// `reserved1`.
    pub reserved1: u8,
    /// `rpm`: media rotation speed.
    pub rpm: [u8; 2],
    /// `reserved2`.
    pub reserved2: u8,
    /// `reserved3`.
    pub reserved3: u8,
}

/// `SPINDLE_SYNCH_MASK`: mask of valid bits.
pub const SPINDLE_SYNCH_MASK: u8 = 0x03;
/// `SPINDLE_SYNCH_NONE`: synch disabled or not supported.
pub const SPINDLE_SYNCH_NONE: u8 = 0x00;
/// `SPINDLE_SYNCH_SLAVE`: disk is a slave.
pub const SPINDLE_SYNCH_SLAVE: u8 = 0x01;
/// `SPINDLE_SYNCH_MASTER`: disk is a master.
pub const SPINDLE_SYNCH_MASTER: u8 = 0x02;
/// `SPINDLE_SYNCH_MCONTROL`: disk is a master control.
pub const SPINDLE_SYNCH_MCONTROL: u8 = 0x03;
/// `struct page_flex_geometry`: the flexible disk geometry mode page.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PageFlexGeometry {
    /// `pg_code`: page code (should be 5).
    pub pg_code: u8,
    /// `pg_length`: page length (should be 0x1a or 0x1e).
    pub pg_length: u8,
    /// `xfr_rate`.
    pub xfr_rate: [u8; 2],
    /// `nheads`: number of heads.
    pub nheads: u8,
    /// `ph_sec_tr`: physical sectors per track.
    pub ph_sec_tr: u8,
    /// `bytes_s`: bytes per sector.
    pub bytes_s: [u8; 2],
    /// `ncyl`: number of cylinders.
    pub ncyl: [u8; 2],
    /// `st_cyl_wp`: start cyl., write precomp.
    pub st_cyl_wp: [u8; 2],
    /// `st_cyl_rwc`: start cyl., red. write cur.
    pub st_cyl_rwc: [u8; 2],
    /// `driv_step`: drive step rate.
    pub driv_step: [u8; 2],
    /// `driv_step_w`: drive step pulse width.
    pub driv_step_w: u8,
    /// `head_settle`: head settle delay.
    pub head_settle: [u8; 2],
    /// `motor_on`: motor on delay.
    pub motor_on: u8,
    /// `motor_off`: motor off delay.
    pub motor_off: u8,
    /// `flags`: various flags: `MOTOR_ON`, `START_AT_SECTOR_1`, `READY_VALID`.
    pub flags: u8,
    /// `step_p_cyl`: step pulses per cylinder.
    pub step_p_cyl: u8,
    /// `write_pre`: write precompensation.
    pub write_pre: u8,
    /// `head_load`: head load delay.
    pub head_load: u8,
    /// `head_unload`: head unload delay.
    pub head_unload: u8,
    /// `pin_34_2`: pin 34 (6) pin 2 (7/11) definition.
    pub pin_34_2: u8,
    /// `pin_4_1`: pin 4 (8/9) pin 1 (13) definition.
    pub pin_4_1: u8,
    /// `rpm`: media rotation speed.
    pub rpm: [u8; 2],
    /// `reserved1`.
    pub reserved1: u8,
    /// `reserved2`.
    pub reserved2: u8,
}

/// `MOTOR_ON`: motor on (pin 16)?.
pub const MOTOR_ON: u8 = 0x20;
/// `START_AT_SECTOR_1`: start at sector 1.
pub const START_AT_SECTOR_1: u8 = 0x40;
/// `READY_VALID`: RDY (pin 34) valid.
pub const READY_VALID: u8 = 0x20;
/// `struct page_reduced_geometry`: the reduced block commands device parameters mode page.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PageReducedGeometry {
    /// `pg_code`: page code (should be 6).
    pub pg_code: u8,
    /// `pg_length`: page length (should be 0x0B).
    pub pg_length: u8,
    /// `wcd`: bit 0 = write cache disable.
    pub wcd: u8,
    /// `bytes_s`: bytes per sector.
    pub bytes_s: [u8; 2],
    /// `sectors`: total number of sectors.
    pub sectors: [u8; 5],
    /// `pow_perf`: power/performance level.
    pub pow_perf: u8,
    /// `flags`: various: `LOCK_DISABLED` ....
    pub flags: u8,
    /// `reserved`.
    pub reserved: u8,
}

/// `LOCK_DISABLED`.
pub const LOCK_DISABLED: u8 = 0x1;
/// `FORMAT_DISABLED`.
pub const FORMAT_DISABLED: u8 = 0x2;
/// `WRITE_DISABLED`.
pub const WRITE_DISABLED: u8 = 0x4;
/// `READ_DISABLED`.
pub const READ_DISABLED: u8 = 0x8;
/// `struct page_caching_mode`: the caching mode page.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PageCachingMode {
    /// `pg_code`: page code (should be 8).
    pub pg_code: u8,
    /// `pg_length`: page length (should be 0x12).
    pub pg_length: u8,
    /// `flags`: `PG_CACHE_FL_*`.
    pub flags: u8,
    /// `priority`: see [`pg_cache_pri_demand`] and [`pg_cache_pri_write`].
    pub priority: u8,
    /// `dis_prefetch_tl`.
    pub dis_prefetch_tl: [u8; 2],
    /// `min_prefetch`.
    pub min_prefetch: [u8; 2],
    /// `max_prefetch`.
    pub max_prefetch: [u8; 2],
    /// `max_prefetch_ceil`.
    pub max_prefetch_ceil: [u8; 2],
}

/// `PG_CACHE_FL_RCD`.
pub const PG_CACHE_FL_RCD: u8 = 1 << 0;
/// `PG_CACHE_FL_MF`.
pub const PG_CACHE_FL_MF: u8 = 1 << 1;
/// `PG_CACHE_FL_WCE`.
pub const PG_CACHE_FL_WCE: u8 = 1 << 2;
/// `PG_CACHE_FL_SIZE`.
pub const PG_CACHE_FL_SIZE: u8 = 1 << 3;
/// `PG_CACHE_FL_DISC`.
pub const PG_CACHE_FL_DISC: u8 = 1 << 4;
/// `PG_CACHE_FL_CAP`.
pub const PG_CACHE_FL_CAP: u8 = 1 << 5;
/// `PG_CACHE_FL_ABPF`.
pub const PG_CACHE_FL_ABPF: u8 = 1 << 6;
/// `PG_CACHE_FL_IC`.
pub const PG_CACHE_FL_IC: u8 = 1 << 7;
/// `PG_CACHE_PRI_DEMAND(_f)`: the demand read retention priority of `page_caching_mode.priority`.
pub const fn pg_cache_pri_demand(f: u8) -> u8 {
    f & 0x0f
}

/// `PG_CACHE_PRI_WRITE(_f)`: the write retention priority of `page_caching_mode.priority`.
pub const fn pg_cache_pri_write(f: u8) -> u8 {
    (f >> 4) & 0x0f
}
/// `SI_PG_DISK_LIMITS`: block limits.
pub const SI_PG_DISK_LIMITS: u8 = 0xb0;
/// `SI_PG_DISK_INFO`: device characteristics.
pub const SI_PG_DISK_INFO: u8 = 0xb1;
/// `SI_PG_DISK_THIN`: thin provisioning.
pub const SI_PG_DISK_THIN: u8 = 0xb2;
/// `struct scsi_vpd_disk_limits`: the block limits VPD page (`SI_PG_DISK_LIMITS`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiVpdDiskLimits {
    /// `hdr`.
    pub hdr: ScsiVpdHdr,
    /// `_reserved1`.
    pub _reserved1: [u8; 1],
    /// `max_comp_wr_len`.
    pub max_comp_wr_len: u8,
    /// `optimal_xfer_granularity`.
    pub optimal_xfer_granularity: [u8; 2],
    /// `max_xfer_len`.
    pub max_xfer_len: [u8; 4],
    /// `optimal_xfer`.
    pub optimal_xfer: [u8; 4],
    /// `max_xd_prefetch_len`.
    pub max_xd_prefetch_len: [u8; 4],
    /// `max_unmap_lba_count`.
    pub max_unmap_lba_count: [u8; 4],
    /// `max_unmap_desc_count`.
    pub max_unmap_desc_count: [u8; 4],
    /// `optimal_unmap_granularity`.
    pub optimal_unmap_granularity: [u8; 4],
    /// `unmap_granularity_align`: `SI_PG_DISK_LIMITS_UGAVALID` is its top bit.
    pub unmap_granularity_align: [u8; 4],
    /// `_reserved2`.
    pub _reserved2: [u8; 28],
}

/// `SI_PG_DISK_LIMITS_LEN`: `scsi_vpd_disk_limits` page length.
pub const SI_PG_DISK_LIMITS_LEN: u8 = 0x10;
/// `SI_PG_DISK_LIMITS_LEN_THIN`: the page length with thin provisioning.
pub const SI_PG_DISK_LIMITS_LEN_THIN: u8 = 0x3c;
/// `SI_PG_DISK_LIMITS_UGAVALID`: `unmap_granularity_align`: the alignment is valid.
pub const SI_PG_DISK_LIMITS_UGAVALID: u32 = 1u32 << 31;
/// `struct scsi_vpd_disk_info`: the block device characteristics VPD page (`SI_PG_DISK_INFO`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ScsiVpdDiskInfo {
    /// `hdr`.
    pub hdr: ScsiVpdHdr,
    /// `rpm`: `VPD_DISK_INFO_RPM_*`.
    pub rpm: [u8; 2],
    /// `_reserved1`.
    pub _reserved1: [u8; 1],
    /// `form_factor`: `VPD_DISK_INFO_FORM_*`.
    pub form_factor: u8,
    /// `_reserved2`.
    pub _reserved2: [u8; 56],
}

impl Default for ScsiVpdDiskInfo {
    fn default() -> Self {
        Self::zeroed()
    }
}

/// `VPD_DISK_INFO_RPM_UNDEF`.
pub const VPD_DISK_INFO_RPM_UNDEF: u16 = 0x0000;
/// `VPD_DISK_INFO_RPM_NONE`.
pub const VPD_DISK_INFO_RPM_NONE: u16 = 0x0001;
/// `VPD_DISK_INFO_FORM_MASK`.
pub const VPD_DISK_INFO_FORM_MASK: u8 = 0xf;
/// `VPD_DISK_INFO_FORM_UNDEF`.
pub const VPD_DISK_INFO_FORM_UNDEF: u8 = 0x0;
/// `VPD_DISK_INFO_FORM_5_25`.
pub const VPD_DISK_INFO_FORM_5_25: u8 = 0x1;
/// `VPD_DISK_INFO_FORM_3_5`.
pub const VPD_DISK_INFO_FORM_3_5: u8 = 0x2;
/// `VPD_DISK_INFO_FORM_2_5`.
pub const VPD_DISK_INFO_FORM_2_5: u8 = 0x3;
/// `VPD_DISK_INFO_FORM_1_8`.
pub const VPD_DISK_INFO_FORM_1_8: u8 = 0x4;
/// `VPD_DISK_INFO_FORM_LT_1_8`.
pub const VPD_DISK_INFO_FORM_LT_1_8: u8 = 0x5;
/// `struct scsi_vpd_disk_thin`: the logical block provisioning VPD page (`SI_PG_DISK_THIN`), followed by a designation descriptor if `VPD_DISK_THIN_DP` is set.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiVpdDiskThin {
    /// `hdr`.
    pub hdr: ScsiVpdHdr,
    /// `threshold_exponent`.
    pub threshold_exponent: u8,
    /// `flags`: `VPD_DISK_THIN_*`.
    pub flags: u8,
    /// `_reserved1`.
    pub _reserved1: [u8; 2],
}

/// `VPD_DISK_THIN_DP`: descriptor present.
pub const VPD_DISK_THIN_DP: u8 = 1 << 0;
/// `VPD_DISK_THIN_ANC_SUP`.
pub const VPD_DISK_THIN_ANC_SUP: u8 = 0x7 << 1;
/// `VPD_DISK_THIN_ANC_SUP_NO`.
pub const VPD_DISK_THIN_ANC_SUP_NO: u8 = 0x0 << 1;
/// `VPD_DISK_THIN_ANC_SUP_YES`.
pub const VPD_DISK_THIN_ANC_SUP_YES: u8 = 0x1 << 1;
/// `VPD_DISK_THIN_TPWS`: WRITE SAME 16.
pub const VPD_DISK_THIN_TPWS: u8 = 1 << 6;
/// `VPD_DISK_THIN_TPU`: UNMAP.
pub const VPD_DISK_THIN_TPU: u8 = 1 << 7;

scsi_wire!(
    ScsiFormatUnit,
    ScsiFormatUnitDefectListHeader,
    ScsiInitializationPatternDescriptor,
    ScsiDefectDescriptorBf,
    ScsiDefectDescriptorBfif,
    ScsiDefectDescriptorPsf,
    ScsiReassignBlocks,
    ScsiRezeroUnit,
    ScsiRw,
    ScsiRw10,
    ScsiRw12,
    ScsiRw16,
    ScsiWriteSame10,
    ScsiWriteSame16,
    ScsiUnmap,
    ScsiUnmapData,
    ScsiUnmapDesc,
    ScsiReadCapacity,
    ScsiReadCapacity16,
    ScsiStartStop,
    ScsiSynchronizeCache,
    ScsiReassignBlocksDefectDescriptor,
    ScsiReassignBlocksData,
    PageDiskFormat,
    PageRigidGeometry,
    PageFlexGeometry,
    PageReducedGeometry,
    PageCachingMode,
    ScsiVpdDiskLimits,
    ScsiVpdDiskInfo,
    ScsiVpdDiskThin,
);

const _: () = {
    assert!(size_of::<ScsiFormatUnit>() == 6);
    assert!(size_of::<ScsiFormatUnitDefectListHeader>() == 4);
    assert!(size_of::<ScsiInitializationPatternDescriptor>() == 4);
    assert!(size_of::<ScsiDefectDescriptorBf>() == 4);
    assert!(size_of::<ScsiDefectDescriptorBfif>() == 5);
    assert!(size_of::<ScsiDefectDescriptorPsf>() == 5);
    assert!(size_of::<ScsiReassignBlocks>() == 6);
    assert!(size_of::<ScsiRezeroUnit>() == 6);
    assert!(size_of::<ScsiRw>() == 6);
    assert!(size_of::<ScsiRw10>() == 10);
    assert!(size_of::<ScsiRw12>() == 12);
    assert!(size_of::<ScsiRw16>() == 16);
    assert!(size_of::<ScsiWriteSame10>() == 10);
    assert!(size_of::<ScsiWriteSame16>() == 16);
    assert!(size_of::<ScsiUnmap>() == 10);
    assert!(size_of::<ScsiUnmapData>() == 8);
    assert!(size_of::<ScsiUnmapDesc>() == 16);
    assert!(size_of::<ScsiReadCapacity>() == 10);
    assert!(size_of::<ScsiReadCapacity16>() == 16);
    assert!(size_of::<ScsiStartStop>() == 6);
    assert!(size_of::<ScsiSynchronizeCache>() == 10);
    assert!(size_of::<ScsiReassignBlocksDefectDescriptor>() == 4);
    assert!(size_of::<ScsiReassignBlocksData>() == 8);
    assert!(size_of::<PageDiskFormat>() == 24);
    assert!(size_of::<PageRigidGeometry>() == 24);
    assert!(size_of::<PageFlexGeometry>() == 32);
    assert!(size_of::<PageReducedGeometry>() == 13);
    assert!(size_of::<PageCachingMode>() == 12);
    assert!(size_of::<ScsiVpdDiskLimits>() == 64);
    assert!(size_of::<ScsiVpdDiskInfo>() == 64);
    assert!(size_of::<ScsiVpdDiskThin>() == 8);
};

#[cfg(test)]
mod tests;
