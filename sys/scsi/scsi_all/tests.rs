//! Host tests for `scsi_all.rs`.

use std::{assert, assert_eq};

use super::*;

#[test]
#[ignore = "needs OPENBSD_SRC (just test-ref)"]
fn values_match_the_c_header() {
    let defs = crate::reftest::defines("sys/scsi/scsi_all.h");
    let ours = crate::reftest::assert_defines!(defs;
            SCSI_CTL_LINK, SCSI_CTL_FLAG, SCSI_CTL_VENDOR, SCSI_CMD_LUN_MASK,
            SCSI_CMD_LUN_SHIFT, SSD_UOL, SSD_DOL, SSD_SELFTEST, SSD_PF, SI_EVPD,
            SI_PG_SUPPORTED, SI_PG_SERIAL, SI_PG_DEVID, SI_PG_ATA, SMS_DBD,
            SMS_PAGE_CODE, SMS_PAGE_CTRL, SMS_PAGE_CTRL_CURRENT,
            SMS_PAGE_CTRL_CHANGEABLE, SMS_PAGE_CTRL_DEFAULT, SMS_PAGE_CTRL_SAVED,
            SMS_LLBAA, SMS_SP, SMS_PF, PR_PREVENT, PR_ALLOW, REPORT_NORMAL,
            REPORT_WELLKNOWN, REPORT_ALL, TEST_UNIT_READY, REQUEST_SENSE, INQUIRY,
            MODE_SELECT, RESERVE, RELEASE, MODE_SENSE, START_STOP, RECEIVE_DIAGNOSTIC,
            SEND_DIAGNOSTIC, PREVENT_ALLOW, POSITION_TO_ELEMENT, WRITE_BUFFER,
            READ_BUFFER, CHANGE_DEFINITION, MODE_SELECT_BIG, MODE_SENSE_BIG,
            REPORT_LUNS, GENRETRY, T_DIRECT, T_SEQUENTIAL, T_PRINTER, T_PROCESSOR,
            T_WORM, T_CDROM, T_SCANNER, T_OPTICAL, T_CHANGER, T_COMM, T_ASC0, T_ASC1,
            T_STORARRAY, T_ENCLOSURE, T_RDIRECT, T_OCRW, T_BCC, T_OSD, T_ADC,
            T_WELL_KNOWN_LU, T_NODEVICE, T_REMOV, T_FIXED, SID_TYPE, SID_QUAL,
            SID_QUAL_LU_OK, SID_QUAL_LU_OFFLINE, SID_QUAL_RSVD, SID_QUAL_BAD_LU,
            SID_QUAL2, SID_REMOVABLE, SID_ANSII, SID_ECMA, SID_ISO,
            SID_RESPONSE_DATA_FMT, SID_SCSI2_RESPONSE, SID_HiSup, SID_NormACA,
            SID_TrmIOP, SID_AENC, SID_SCSI2_HDRLEN, SID_SCSI2_ALEN, SPC3_SID_PROTECT,
            SPC3_SID_RESERVED, SPC3_SID_3PC, SPC3_SID_TPGS_IMPLICIT,
            SPC3_SID_TPGS_EXPLICIT, SPC3_SID_ACC, SPC3_SID_SCCS, SPC2_SID_ADDR16,
            SPC2_RESERVED, SPC2_SID_NChngr, SPC2_SID_MultiP, SPC2_VS, SPC2_SID_EncServ,
            SPC2_SID_BQueue, SID_VS, SID_CmdQue, SID_Linked, SID_Sync, SID_WBus16,
            SID_WBus32, SID_RelAdr, VPD_PROTO_ID_FC, VPD_PROTO_ID_SPI, VPD_PROTO_ID_SSA,
            VPD_PROTO_ID_IEEE1394, VPD_PROTO_ID_SRP, VPD_PROTO_ID_ISCSI,
            VPD_PROTO_ID_SAS, VPD_PROTO_ID_ADT, VPD_PROTO_ID_ATA, VPD_PROTO_ID_NONE,
            VPD_DEVID_CODE_BINARY, VPD_DEVID_CODE_ASCII, VPD_DEVID_CODE_UTF8,
            VPD_DEVID_PIV, VPD_DEVID_ASSOC_LU, VPD_DEVID_ASSOC_PORT,
            VPD_DEVID_ASSOC_TARG, VPD_DEVID_TYPE_VENDOR, VPD_DEVID_TYPE_T10,
            VPD_DEVID_TYPE_EUI64, VPD_DEVID_TYPE_NAA, VPD_DEVID_TYPE_RELATIVE,
            VPD_DEVID_TYPE_PORT, VPD_DEVID_TYPE_LU, VPD_DEVID_TYPE_MD5,
            VPD_DEVID_TYPE_NAME, VPD_ATA_COMMAND_CODE_ATA, VPD_ATA_COMMAND_CODE_ATAPI,
            RC16_PROT_EN, RC16_PROT_P_TYPE, RC16_P_TYPE_1, RC16_P_TYPE_2, RC16_P_TYPE_3,
            RC16_BASIS, RC16_BASIS_HIGH, RC16_BASIS_LAST, RC16_LBPPB_EXPONENT,
            RC16_PIIPLB_EXPONENT, RC16_LALBA, RC16_LBPRZ, READ_CAP_16_TPRZ, RC16_LBPME,
            READ_CAP_16_TPE, SSD_ERRCODE_CURRENT, SSD_ERRCODE_DEFERRED, SSD_ERRCODE,
            SSD_ERRCODE_VALID, SSD_KEY, SSD_ILI, SSD_EOM, SSD_FILEMARK, SSD_SCS_VALID,
            SSD_SCS_CDB_ERROR, SSD_SCS_SEGMENT_DESC, SSD_SCS_VALID_BIT_INDEX,
            SSD_SCS_BIT_INDEX, SKEY_NO_SENSE, SKEY_RECOVERED_ERROR, SKEY_NOT_READY,
            SKEY_MEDIUM_ERROR, SKEY_HARDWARE_ERROR, SKEY_ILLEGAL_REQUEST,
            SKEY_UNIT_ATTENTION, SKEY_WRITE_PROTECT, SKEY_BLANK_CHECK,
            SKEY_VENDOR_UNIQUE, SKEY_COPY_ABORTED, SKEY_ABORTED_COMMAND, SKEY_EQUAL,
            SKEY_VOLUME_OVERFLOW, SKEY_MISCOMPARE, SKEY_RESERVED,
            SENSE_FILEMARK_DETECTED, SENSE_END_OF_MEDIUM_DETECTED,
            SENSE_SETMARK_DETECTED, SENSE_BEGINNING_OF_MEDIUM_DETECTED,
            SENSE_END_OF_DATA_DETECTED, SENSE_NOT_READY_BECOMING_READY,
            SENSE_NOT_READY_INIT_REQUIRED, SENSE_NOT_READY_FORMAT,
            SENSE_NOT_READY_REBUILD, SENSE_NOT_READY_RECALC, SENSE_NOT_READY_INPROGRESS,
            SENSE_NOT_READY_LONGWRITE, SENSE_NOT_READY_SELFTEST,
            SENSE_POWER_RESET_OR_BUS, SENSE_POWER_ON, SENSE_BUS_RESET,
            SENSE_BUS_DEVICE_RESET, SENSE_DEVICE_INTERNAL_RESET, SENSE_TSC_CHANGE_SE,
            SENSE_TSC_CHANGE_LVD, SENSE_IT_NEXUS_LOSS, SENSE_BAD_MEDIUM,
            SENSE_NR_MEDIUM_UNKNOWN_FORMAT, SENSE_NR_MEDIUM_INCOMPATIBLE_FORMAT,
            SENSE_NW_MEDIUM_UNKNOWN_FORMAT, SENSE_NW_MEDIUM_INCOMPATIBLE_FORMAT,
            SENSE_NF_MEDIUM_INCOMPATIBLE_FORMAT, SENSE_NW_MEDIUM_AC_MISMATCH,
            SENSE_NOMEDIUM, SENSE_NOMEDIUM_TCLOSED, SENSE_NOMEDIUM_TOPEN,
            SENSE_NOMEDIUM_LOADABLE, SENSE_NOMEDIUM_AUXMEM, SENSE_CARTRIDGE_FAULT,
            SENSE_MEDIUM_REMOVAL_PREVENTED, LONGLBA, SMH_DSP_WRITE_PROT,
            RPL_LUNDATA_SIZE, RPL_LUNDATA_T0LUN, ATA_PASSTHRU_12, ATA_PASSTHRU_16,
            ATA_PASSTHRU_PROTO_MASK, ATA_PASSTHRU_PROTO_HW_RESET,
            ATA_PASSTHRU_PROTO_SW_RESET, ATA_PASSTHRU_PROTO_NON_DATA,
            ATA_PASSTHRU_PROTO_PIO_DATAIN, ATA_PASSTHRU_PROTO_PIO_DATAOUT,
            ATA_PASSTHRU_PROTO_DMA, ATA_PASSTHRU_PROTO_DMA_QUEUED,
            ATA_PASSTHRU_PROTO_EXEC_DIAG, ATA_PASSTHRU_PROTO_NON_DATA_RST,
            ATA_PASSTHRU_PROTO_UDMA_DATAIN, ATA_PASSTHRU_PROTO_UDMA_DATAOUT,
            ATA_PASSTHRU_PROTO_FPDMA, ATA_PASSTHRU_PROTO_RESPONSE,
            ATA_PASSTHRU_T_DIR_MASK, ATA_PASSTHRU_T_DIR_READ, ATA_PASSTHRU_T_DIR_WRITE,
            ATA_PASSTHRU_T_LEN_MASK, ATA_PASSTHRU_T_LEN_NONE,
            ATA_PASSTHRU_T_LEN_FEATURES, ATA_PASSTHRU_T_LEN_SECTOR_COUNT,
            ATA_PASSTHRU_T_LEN_TPSIU, SIU_SNSVALID, SIU_RSPVALID, SIU_PFC_NONE,
            SIU_PFC_CIU_FIELDS_INVALID, SIU_PFC_TMF_NOT_SUPPORTED, SIU_PFC_TMF_FAILED,
            SIU_PFC_INVALID_TYPE_CODE, SIU_PFC_ILLEGAL_REQUEST, SIU_TASKMGMT_NONE,
            SIU_TASKMGMT_ABORT_TASK, SIU_TASKMGMT_ABORT_TASK_SET,
            SIU_TASKMGMT_CLEAR_TASK_SET, SIU_TASKMGMT_LUN_RESET,
            SIU_TASKMGMT_TARGET_RESET, SIU_TASKMGMT_CLEAR_ACA, SCSI_OK, SCSI_CHECK,
            SCSI_COND_MET, SCSI_BUSY, SCSI_INTERM, SCSI_INTERM_COND_MET,
            SCSI_RESV_CONFLICT, SCSI_TERMINATED, SCSI_QUEUE_FULL, SCSI_TASKSET_FULL,
            SCSI_ACA_ACTIVE,
    );
    crate::reftest::assert_complete(&defs, "SKEY_", &ours);
    crate::reftest::assert_complete(&defs, "SENSE_", &ours);
    crate::reftest::assert_complete(&defs, "T_", &ours);
}

#[test]
fn wire_views_alias_the_bytes() {
    let mut cmd = ScsiGeneric::zeroed();
    let inq: &mut ScsiInquiry = wire_mut(cmd.as_bytes_mut());
    inq.opcode = INQUIRY;
    inq.flags = SI_EVPD;
    inq.pagecode = SI_PG_SERIAL;
    inq.length = [0x01, 0x02];
    assert_eq!(cmd.opcode, INQUIRY);
    assert_eq!(&cmd.bytes[..4], &[SI_EVPD, SI_PG_SERIAL, 0x01, 0x02]);
    let back: &ScsiInquiry = wire_ref(cmd.as_bytes());
    assert_eq!(back.length, [0x01, 0x02]);
}

#[test]
fn read_from_zero_fills_a_short_source() {
    let d = ScsiReadCapData::read_from(&[1, 2, 3]);
    assert_eq!(d.addr, [1, 2, 3, 0]);
    assert_eq!(d.length, [0; 4]);
}

#[test]
fn mode_sense_buf_headers_overlay_the_front() {
    let mut b = ScsiModeSenseBuf::new();
    b.hdr_mut().data_length = 3;
    assert!(valid_mode_hdr(b.hdr()));
    b.buf[0] = 0;
    b.buf[1] = 6;
    assert!(valid_mode_hdr_big(b.hdr_big()));
    b.buf[1] = 5;
    assert!(!valid_mode_hdr_big(b.hdr_big()));
}

#[test]
fn macros_as_functions() {
    let mut s = ScsiSenseData::new();
    s.add_sense_code = 0x3a;
    s.add_sense_code_qual = 0x02;
    assert_eq!(asc_ascq(&s), SENSE_NOMEDIUM_TOPEN);
    assert_eq!(vpd_devid_pi(0x63), 6);
    assert_eq!(vpd_devid_code(0x63), 3);
    assert_eq!(vpd_devid_assoc(0x93), VPD_DEVID_ASSOC_PORT);
    assert_eq!(vpd_devid_type(0x93), VPD_DEVID_TYPE_NAA);

    let mut iu = [0u8; 32];
    iu[2] = SIU_RSPVALID;
    iu[11] = 4; // pkt_failures_length
    iu[12 + 3] = SIU_PFC_TMF_FAILED;
    let hdr: &ScsiStatusIuHeader = wire_ref(&iu);
    assert_eq!(siu_pktfail_code(&iu), SIU_PFC_TMF_FAILED);
    assert_eq!(siu_sense_data_offset(hdr), 4);
    iu[2] = 0;
    let hdr: &ScsiStatusIuHeader = wire_ref(&iu);
    assert_eq!(siu_sense_data_offset(hdr), 0);
}
