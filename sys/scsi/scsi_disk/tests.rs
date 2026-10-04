//! Host tests for `scsi_disk.rs`.

use core::mem::offset_of;
use std::assert_eq;

use super::*;
use crate::scsi::scsi_all::{ScsiGeneric, wire_mut, wire_ref};

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let mut defs = crate::reftest::defines("sys/scsi/scsi_disk.h");
    // The include guard is no constant.
    defs.remove("_SCSI_SCSI_DISK_H");
    let ours = crate::reftest::assert_defines!(defs;
            FORMAT_UNIT, SFU_DLF_MASK, SFU_CMPLST, SFU_FMTDATA, DLH_VS,
            DLH_IMMED, DLH_DSP, DLH_IP, DLH_STPF, DLH_DCRT, DLH_DPRY,
            DLH_FOV, IP_TYPE_DEFAULT, IP_TYPE_REPEAT, REZERO_UNIT,
            SRW_TOPADDR, SRWB_RELADDR, WRITE_SAME_F_LBDATA,
            WRITE_SAME_F_PBDATA, WRITE_SAME_F_UNMAP, WRITE_SAME_F_ANCHOR,
            SRC16_SERVICE_ACTION, SSS_STOP, SSS_START, SSS_LOEJ, SSC_RELADR,
            SSC_IMMED, REASSIGN_BLOCKS, READ_COMMAND, WRITE_COMMAND,
            READ_CAPACITY, READ_CAPACITY_16, READ_10, WRITE_10, READ_12,
            WRITE_12, READ_16, WRITE_16, SYNCHRONIZE_CACHE, WRITE_SAME_10,
            WRITE_SAME_16, UNMAP, PAGE_DISK_FORMAT, PAGE_RIGID_GEOMETRY,
            PAGE_FLEX_GEOMETRY, PAGE_REDUCED_GEOMETRY, PAGE_CACHING_MODE,
            DISK_FMT_SURF, DISK_FMT_RMB, DISK_FMT_HSEC, DISK_FMT_SSEC,
            SPINDLE_SYNCH_MASK, SPINDLE_SYNCH_NONE, SPINDLE_SYNCH_SLAVE,
            SPINDLE_SYNCH_MASTER, SPINDLE_SYNCH_MCONTROL, MOTOR_ON,
            START_AT_SECTOR_1, READY_VALID, LOCK_DISABLED, FORMAT_DISABLED,
            WRITE_DISABLED, READ_DISABLED, PG_CACHE_FL_RCD, PG_CACHE_FL_MF,
            PG_CACHE_FL_WCE, PG_CACHE_FL_SIZE, PG_CACHE_FL_DISC,
            PG_CACHE_FL_CAP, PG_CACHE_FL_ABPF, PG_CACHE_FL_IC,
            SI_PG_DISK_LIMITS, SI_PG_DISK_INFO, SI_PG_DISK_THIN,
            SI_PG_DISK_LIMITS_LEN, SI_PG_DISK_LIMITS_LEN_THIN,
            SI_PG_DISK_LIMITS_UGAVALID, VPD_DISK_INFO_RPM_UNDEF,
            VPD_DISK_INFO_RPM_NONE, VPD_DISK_INFO_FORM_MASK,
            VPD_DISK_INFO_FORM_UNDEF, VPD_DISK_INFO_FORM_5_25,
            VPD_DISK_INFO_FORM_3_5, VPD_DISK_INFO_FORM_2_5,
            VPD_DISK_INFO_FORM_1_8, VPD_DISK_INFO_FORM_LT_1_8,
            VPD_DISK_THIN_DP, VPD_DISK_THIN_ANC_SUP,
            VPD_DISK_THIN_ANC_SUP_NO, VPD_DISK_THIN_ANC_SUP_YES,
            VPD_DISK_THIN_TPWS, VPD_DISK_THIN_TPU,
    );
    crate::reftest::assert_complete(&defs, "", &ours);
}

#[test]
fn cdb_layouts_follow_the_c_offsets() {
    assert_eq!(offset_of!(ScsiRw, length), 4);
    assert_eq!(offset_of!(ScsiRw10, addr), 2);
    assert_eq!(offset_of!(ScsiRw10, length), 7);
    assert_eq!(offset_of!(ScsiRw12, length), 6);
    assert_eq!(offset_of!(ScsiRw16, addr), 2);
    assert_eq!(offset_of!(ScsiRw16, length), 10);
    assert_eq!(offset_of!(ScsiReadCapacity16, length), 10);
    assert_eq!(offset_of!(ScsiStartStop, how), 4);
    assert_eq!(offset_of!(ScsiSynchronizeCache, length), 7);
    assert_eq!(offset_of!(ScsiWriteSame16, group_number), 14);
    assert_eq!(offset_of!(ScsiUnmap, list_len), 7);
    assert_eq!(offset_of!(ScsiReassignBlocksData, defect_descriptor), 4);
}

#[test]
fn page_layouts_follow_the_c_offsets() {
    assert_eq!(offset_of!(PageDiskFormat, bytes_s), 12);
    assert_eq!(offset_of!(PageDiskFormat, flags), 20);
    assert_eq!(offset_of!(PageRigidGeometry, nheads), 5);
    assert_eq!(offset_of!(PageRigidGeometry, rpm), 20);
    assert_eq!(offset_of!(PageFlexGeometry, rpm), 28);
    assert_eq!(offset_of!(PageReducedGeometry, sectors), 5);
    assert_eq!(offset_of!(PageCachingMode, max_prefetch_ceil), 10);
    assert_eq!(offset_of!(ScsiVpdDiskLimits, max_xfer_len), 8);
    assert_eq!(offset_of!(ScsiVpdDiskLimits, max_unmap_lba_count), 20);
    assert_eq!(offset_of!(ScsiVpdDiskLimits, unmap_granularity_align), 32);
    assert_eq!(offset_of!(ScsiVpdDiskInfo, form_factor), 7);
    assert_eq!(offset_of!(ScsiVpdDiskThin, flags), 5);
}

#[test]
fn cdbs_overlay_the_generic_command() {
    let mut cmd = ScsiGeneric::zeroed();
    let rw: &mut ScsiRw10 = wire_mut(cmd.as_bytes_mut());
    rw.opcode = READ_10;
    rw.addr = [0x01, 0x02, 0x03, 0x04];
    rw.length = [0x00, 0x08];
    assert_eq!(&cmd.bytes[1..8], &[1, 2, 3, 4, 0, 0, 8]);
    let back: &ScsiRw10 = wire_ref(cmd.as_bytes());
    assert_eq!(back.length, [0x00, 0x08]);
}

#[test]
fn caching_priorities_split_the_nibbles() {
    assert_eq!(pg_cache_pri_demand(0xa5), 0x5);
    assert_eq!(pg_cache_pri_write(0xa5), 0xa);
}

#[test]
fn thin_flags_compose() {
    let f = VPD_DISK_THIN_TPU | VPD_DISK_THIN_ANC_SUP_YES | VPD_DISK_THIN_DP;
    assert_eq!(f & VPD_DISK_THIN_ANC_SUP, VPD_DISK_THIN_ANC_SUP_YES);
    assert_eq!(f & VPD_DISK_THIN_TPWS, 0);
    assert_eq!(SI_PG_DISK_LIMITS_UGAVALID, 0x8000_0000);
}
