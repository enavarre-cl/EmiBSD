/* <LICENSES> */
/*
 * SCSI general  interface description
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

/* <CODE> */
//! `<scsi/scsi_all.h>`: the SCSI command, status and data formats every device class shares:
//! the generic command descriptor block, the commands of the primary command set (TEST UNIT
//! READY, INQUIRY, MODE SENSE/SELECT, PREVENT ALLOW, REPORT LUNS, ...), the INQUIRY and VPD
//! data, the READ CAPACITY data, the sense data with its keys and additional sense codes, the
//! mode parameter headers and block descriptors, ATA PASS-THROUGH and the status byte.
//!
//! Upstream: sys/scsi/scsi_all.h @ 3ce1f3f79392
//!
//! Every structure here is a wire format: `#[repr(C)]` made of `u8`s and byte arrays only, so
//! its alignment is 1, it has no padding and every bit pattern is a valid value. [`ScsiWire`]
//! states that once per type and gives the byte views the C gets by casting pointers
//! (`(struct scsi_inquiry *)&xs->cmd`): [`wire_ref`] and [`wire_mut`] view the front of a byte
//! slice as a structure, `as_bytes`/`as_bytes_mut` view a structure as bytes. The sizes are
//! pinned at compile time at the end of the file.
//!
//! ## Deviations
//! - `union scsi_mode_sense_buf` (`__packed`, 254 bytes) is [`ScsiModeSenseBuf`], the byte
//!   array `buf` with [`hdr`](ScsiModeSenseBuf::hdr)/[`hdr_big`](ScsiModeSenseBuf::hdr_big)
//!   (and `_mut`) views: every member of the union is bytes, so the views are the union.
//! - Function-like macros are `const fn`s with lower-case names (`VPD_DEVID_PI` is
//!   [`vpd_devid_pi`], `ASC_ASCQ` is [`asc_ascq`], `VALID_MODE_HDR` is [`valid_mode_hdr`]).
//!   `SIU_PKTFAIL_CODE` and `SIU_SENSE_DATA` index past the end of `struct
//!   scsi_status_iu_header` (its `data[1]` is a variable-length tail), so they take the whole
//!   information unit as a byte slice ([`siu_pktfail_code`]) or return the offset of the sense
//!   data in it ([`siu_sense_data_offset`]).
//! - `char vendor[8]` and the other `char` arrays of the INQUIRY data are `[u8; N]`: they are
//!   bytes on the wire, not C strings.
//! - `struct scsi_lun_array` holds two `int`s besides its bytes; it is no wire format, so it is
//!   a plain structure and not a [`ScsiWire`].

use core::mem::size_of;
use core::slice;

use crate::scsi::scsiconf::{_2btol, _4btol};

/*
 * SCSI command format
 */

/*
 * Define some bits that are in ALL (or a lot of) scsi commands
 */
/// `SCSI_CTL_LINK`.
pub const SCSI_CTL_LINK: u8 = 0x01;
/// `SCSI_CTL_FLAG`.
pub const SCSI_CTL_FLAG: u8 = 0x02;
/// `SCSI_CTL_VENDOR`.
pub const SCSI_CTL_VENDOR: u8 = 0xC0;

/// `SCSI_CMD_LUN_MASK`: some old SCSI devices need the LUN in the top 3 bits of the second
/// byte of the CDB.
pub const SCSI_CMD_LUN_MASK: u8 = 0xe0;
/// `SCSI_CMD_LUN_SHIFT`.
pub const SCSI_CMD_LUN_SHIFT: u8 = 5;

/// `SSD_UOL` (`scsi_send_diag.byte2`).
pub const SSD_UOL: u8 = 0x01;
/// `SSD_DOL`.
pub const SSD_DOL: u8 = 0x02;
/// `SSD_SELFTEST`.
pub const SSD_SELFTEST: u8 = 0x04;
/// `SSD_PF`.
pub const SSD_PF: u8 = 0x10;

/// `SI_EVPD` (`scsi_inquiry.flags`): ask for a vital product data page.
pub const SI_EVPD: u8 = 0x01;
/// `SI_PG_SUPPORTED`: the supported VPD pages page.
pub const SI_PG_SUPPORTED: u8 = 0x00;
/// `SI_PG_SERIAL`: the unit serial number page.
pub const SI_PG_SERIAL: u8 = 0x80;
/// `SI_PG_DEVID`: the device identification page.
pub const SI_PG_DEVID: u8 = 0x83;
/// `SI_PG_ATA`: the ATA information page.
pub const SI_PG_ATA: u8 = 0x89;

/// `SMS_DBD` (`scsi_mode_sense.byte2`): disable block descriptors.
pub const SMS_DBD: u8 = 0x08;
/// `SMS_PAGE_CODE` (`scsi_mode_sense.page`).
pub const SMS_PAGE_CODE: u8 = 0x3F;
/// `SMS_PAGE_CTRL`.
pub const SMS_PAGE_CTRL: u8 = 0xC0;
/// `SMS_PAGE_CTRL_CURRENT`.
pub const SMS_PAGE_CTRL_CURRENT: u8 = 0x00;
/// `SMS_PAGE_CTRL_CHANGEABLE`.
pub const SMS_PAGE_CTRL_CHANGEABLE: u8 = 0x40;
/// `SMS_PAGE_CTRL_DEFAULT`.
pub const SMS_PAGE_CTRL_DEFAULT: u8 = 0x80;
/// `SMS_PAGE_CTRL_SAVED`.
pub const SMS_PAGE_CTRL_SAVED: u8 = 0xC0;
/// `SMS_LLBAA` (`scsi_mode_sense_big.byte2`): long LBA accepted.
pub const SMS_LLBAA: u8 = 0x10;
/// `SMS_SP` (`scsi_mode_select.byte2`): save pages.
pub const SMS_SP: u8 = 0x01;
/// `SMS_PF`: page format.
pub const SMS_PF: u8 = 0x10;

/// `PR_PREVENT` (`scsi_prevent.how`).
pub const PR_PREVENT: u8 = 0x01;
/// `PR_ALLOW`.
pub const PR_ALLOW: u8 = 0x00;

/// `REPORT_NORMAL` (`scsi_report_luns.selectreport`).
pub const REPORT_NORMAL: u8 = 0x00;
/// `REPORT_WELLKNOWN`.
pub const REPORT_WELLKNOWN: u8 = 0x01;
/// `REPORT_ALL`.
pub const REPORT_ALL: u8 = 0x02;

/*
 * Opcodes
 */
/// `TEST_UNIT_READY`.
pub const TEST_UNIT_READY: u8 = 0x00;
/// `REQUEST_SENSE`.
pub const REQUEST_SENSE: u8 = 0x03;
/// `INQUIRY`.
pub const INQUIRY: u8 = 0x12;
/// `MODE_SELECT`.
pub const MODE_SELECT: u8 = 0x15;
/// `RESERVE`.
pub const RESERVE: u8 = 0x16;
/// `RELEASE`.
pub const RELEASE: u8 = 0x17;
/// `MODE_SENSE`.
pub const MODE_SENSE: u8 = 0x1a;
/// `START_STOP`.
pub const START_STOP: u8 = 0x1b;
/// `RECEIVE_DIAGNOSTIC`.
pub const RECEIVE_DIAGNOSTIC: u8 = 0x1c;
/// `SEND_DIAGNOSTIC`.
pub const SEND_DIAGNOSTIC: u8 = 0x1d;
/// `PREVENT_ALLOW`.
pub const PREVENT_ALLOW: u8 = 0x1e;
/// `POSITION_TO_ELEMENT`.
pub const POSITION_TO_ELEMENT: u8 = 0x2b;
/// `WRITE_BUFFER`.
pub const WRITE_BUFFER: u8 = 0x3b;
/// `READ_BUFFER`.
pub const READ_BUFFER: u8 = 0x3c;
/// `CHANGE_DEFINITION`.
pub const CHANGE_DEFINITION: u8 = 0x40;
/// `MODE_SELECT_BIG`: MODE SELECT (10).
pub const MODE_SELECT_BIG: u8 = 0x55;
/// `MODE_SENSE_BIG`: MODE SENSE (10).
pub const MODE_SENSE_BIG: u8 = 0x5a;
/// `REPORT_LUNS`.
pub const REPORT_LUNS: u8 = 0xa0;

/// `GENRETRY`: sort of an extra one, for `SCSI_RESET`.
pub const GENRETRY: i32 = 1;

/*
 * Device Types
 */
/// `T_DIRECT`: direct access block device (SBC-4).
pub const T_DIRECT: u8 = 0x00;
/// `T_SEQUENTIAL`: sequential access device (SSC-3).
pub const T_SEQUENTIAL: u8 = 0x01;
/// `T_PRINTER`: printer device (SSC).
pub const T_PRINTER: u8 = 0x02;
/// `T_PROCESSOR`: processor device (SPC-2).
pub const T_PROCESSOR: u8 = 0x03;
/// `T_WORM`: write once device (SBC).
pub const T_WORM: u8 = 0x04;
/// `T_CDROM`: CD/DVD device (MMC-5).
pub const T_CDROM: u8 = 0x05;
/// `T_SCANNER`: scanner device (obsolete).
pub const T_SCANNER: u8 = 0x06;
/// `T_OPTICAL`: optical memory device (SBC).
pub const T_OPTICAL: u8 = 0x07;
/// `T_CHANGER`: media changer device (SMC-3).
pub const T_CHANGER: u8 = 0x08;
/// `T_COMM`: communications device (obsolete).
pub const T_COMM: u8 = 0x09;
/// `T_ASC0`: obsolete.
pub const T_ASC0: u8 = 0x0a;
/// `T_ASC1`: obsolete.
pub const T_ASC1: u8 = 0x0b;
/// `T_STORARRAY`: storage array controller device (RAID).
pub const T_STORARRAY: u8 = 0x0c;
/// `T_ENCLOSURE`: enclosure services device (SES).
pub const T_ENCLOSURE: u8 = 0x0d;
/// `T_RDIRECT`: simplified direct access device (RBC).
pub const T_RDIRECT: u8 = 0x0e;
/// `T_OCRW`: optical card reader/writer (OCRW).
pub const T_OCRW: u8 = 0x0f;
/// `T_BCC`: bridge controller commands (BCC).
pub const T_BCC: u8 = 0x10;
/// `T_OSD`: object-based storage device (OSD).
pub const T_OSD: u8 = 0x11;
/// `T_ADC`: automation/drive interface (ADC-2).
pub const T_ADC: u8 = 0x12;
/* 0x13 - 0x1d RESERVED */
/// `T_WELL_KNOWN_LU`: well known logical unit.
pub const T_WELL_KNOWN_LU: u8 = 0x1e;
/// `T_NODEVICE`: unknown or no device type.
pub const T_NODEVICE: u8 = 0x1F;

/// `T_REMOV`: removable (`scsi_inquiry_pattern.removable`).
pub const T_REMOV: i32 = 1;
/// `T_FIXED`: fixed.
pub const T_FIXED: i32 = 0;

/// `SID_TYPE` (`scsi_inquiry_data.device`): the peripheral device type.
pub const SID_TYPE: u8 = 0x1f;
/// `SID_QUAL`: the peripheral qualifier.
pub const SID_QUAL: u8 = 0xe0;
/// `SID_QUAL_LU_OK`.
pub const SID_QUAL_LU_OK: u8 = 0x00;
/// `SID_QUAL_LU_OFFLINE`.
pub const SID_QUAL_LU_OFFLINE: u8 = 0x20;
/// `SID_QUAL_RSVD`.
pub const SID_QUAL_RSVD: u8 = 0x40;
/// `SID_QUAL_BAD_LU`.
pub const SID_QUAL_BAD_LU: u8 = 0x60;
/// `SID_QUAL2` (`dev_qual2`).
pub const SID_QUAL2: u8 = 0x7f;
/// `SID_REMOVABLE`.
pub const SID_REMOVABLE: u8 = 0x80;
/// `SID_ANSII` (`version`).
pub const SID_ANSII: u8 = 0x07;
/// `SID_ECMA`.
pub const SID_ECMA: u8 = 0x38;
/// `SID_ISO`.
pub const SID_ISO: u8 = 0xc0;
/// `SID_RESPONSE_DATA_FMT` (`response_format`): < 2 is obsolete, > 2 reserved.
pub const SID_RESPONSE_DATA_FMT: u8 = 0x0f;
/// `SID_SCSI2_RESPONSE`.
pub const SID_SCSI2_RESPONSE: u8 = 0x02;
/// `SID_HiSup`: hierarchical LUNs.
#[allow(non_upper_case_globals)] // the C name
pub const SID_HiSup: u8 = 0x10;
/// `SID_NormACA`: normal ACA bit in CCB supported.
#[allow(non_upper_case_globals)] // the C name
pub const SID_NormACA: u8 = 0x20;
/// `SID_TrmIOP`: obsolete.
#[allow(non_upper_case_globals)] // the C name
pub const SID_TrmIOP: u8 = 0x40;
/// `SID_AENC`: obsolete.
pub const SID_AENC: u8 = 0x80;
/// `SID_SCSI2_HDRLEN`: bytes up to and including `additional_length`.
pub const SID_SCSI2_HDRLEN: usize = 5;
/// `SID_SCSI2_ALEN`: additional bytes of basic SCSI-2 info.
pub const SID_SCSI2_ALEN: usize = 31;
/// `SPC3_SID_PROTECT` (`spc3_flags`): 0 is type 0, 1 is type 1, 2 or 3.
pub const SPC3_SID_PROTECT: u8 = 0x01;
/// `SPC3_SID_RESERVED`.
pub const SPC3_SID_RESERVED: u8 = 0x06;
/// `SPC3_SID_3PC`: third party copy.
pub const SPC3_SID_3PC: u8 = 0x08;
/// `SPC3_SID_TPGS_IMPLICIT`: implicit asymmetric LU access.
pub const SPC3_SID_TPGS_IMPLICIT: u8 = 0x10;
/// `SPC3_SID_TPGS_EXPLICIT`: explicit asymmetric LU access.
pub const SPC3_SID_TPGS_EXPLICIT: u8 = 0x20;
/// `SPC3_SID_ACC`: access controls controller.
pub const SPC3_SID_ACC: u8 = 0x40;
/// `SPC3_SID_SCCS`: embedded storage array controller.
pub const SPC3_SID_SCCS: u8 = 0x80;
/// `SPC2_SID_ADDR16` (`spc2_flags`): obsolete.
pub const SPC2_SID_ADDR16: u8 = 0x01;
/// `SPC2_RESERVED`.
pub const SPC2_RESERVED: u8 = 0x06;
/// `SPC2_SID_NChngr`: obsolete.
#[allow(non_upper_case_globals)] // the C name
pub const SPC2_SID_NChngr: u8 = 0x08;
/// `SPC2_SID_MultiP`: multi-port target.
#[allow(non_upper_case_globals)] // the C name
pub const SPC2_SID_MultiP: u8 = 0x10;
/// `SPC2_VS`.
pub const SPC2_VS: u8 = 0x20;
/// `SPC2_SID_EncServ`: embedded enclosure services.
#[allow(non_upper_case_globals)] // the C name
pub const SPC2_SID_EncServ: u8 = 0x40;
/// `SPC2_SID_BQueue`: obsolete.
#[allow(non_upper_case_globals)] // the C name
pub const SPC2_SID_BQueue: u8 = 0x80;
/// `SID_VS` (`flags`).
pub const SID_VS: u8 = 0x01;
/// `SID_CmdQue`: task management mode supported.
#[allow(non_upper_case_globals)] // the C name
pub const SID_CmdQue: u8 = 0x02;
/// `SID_Linked`: obsolete.
#[allow(non_upper_case_globals)] // the C name
pub const SID_Linked: u8 = 0x08;
/// `SID_Sync`: obsolete.
#[allow(non_upper_case_globals)] // the C name
pub const SID_Sync: u8 = 0x10;
/// `SID_WBus16`: obsolete.
#[allow(non_upper_case_globals)] // the C name
pub const SID_WBus16: u8 = 0x20;
/// `SID_WBus32`: obsolete.
#[allow(non_upper_case_globals)] // the C name
pub const SID_WBus32: u8 = 0x40;
/// `SID_RelAdr`: obsolete.
#[allow(non_upper_case_globals)] // the C name
pub const SID_RelAdr: u8 = 0x80;

/// `VPD_PROTO_ID_FC`: Fibre Channel.
pub const VPD_PROTO_ID_FC: u8 = 0x0;
/// `VPD_PROTO_ID_SPI`: parallel SCSI.
pub const VPD_PROTO_ID_SPI: u8 = 0x1;
/// `VPD_PROTO_ID_SSA`.
pub const VPD_PROTO_ID_SSA: u8 = 0x2;
/// `VPD_PROTO_ID_IEEE1394`.
pub const VPD_PROTO_ID_IEEE1394: u8 = 0x3;
/// `VPD_PROTO_ID_SRP`: SCSI RDMA protocol.
pub const VPD_PROTO_ID_SRP: u8 = 0x4;
/// `VPD_PROTO_ID_ISCSI`: internet SCSI.
pub const VPD_PROTO_ID_ISCSI: u8 = 0x5;
/// `VPD_PROTO_ID_SAS`: serial attached SCSI.
pub const VPD_PROTO_ID_SAS: u8 = 0x6;
/// `VPD_PROTO_ID_ADT`: automation/drive interface transport.
pub const VPD_PROTO_ID_ADT: u8 = 0x7;
/// `VPD_PROTO_ID_ATA`: ATA/ATAPI.
pub const VPD_PROTO_ID_ATA: u8 = 0x7;
/// `VPD_PROTO_ID_NONE`.
pub const VPD_PROTO_ID_NONE: u8 = 0xf;

/// `VPD_DEVID_CODE_BINARY` (`scsi_vpd_devid_hdr.pi_code`).
pub const VPD_DEVID_CODE_BINARY: u8 = 0x1;
/// `VPD_DEVID_CODE_ASCII`.
pub const VPD_DEVID_CODE_ASCII: u8 = 0x2;
/// `VPD_DEVID_CODE_UTF8`.
pub const VPD_DEVID_CODE_UTF8: u8 = 0x3;
/// `VPD_DEVID_PIV` (`scsi_vpd_devid_hdr.flags`).
pub const VPD_DEVID_PIV: u8 = 0x80;
/// `VPD_DEVID_ASSOC_LU`.
pub const VPD_DEVID_ASSOC_LU: u8 = 0x00;
/// `VPD_DEVID_ASSOC_PORT`.
pub const VPD_DEVID_ASSOC_PORT: u8 = 0x10;
/// `VPD_DEVID_ASSOC_TARG`.
pub const VPD_DEVID_ASSOC_TARG: u8 = 0x20;
/// `VPD_DEVID_TYPE_VENDOR`.
pub const VPD_DEVID_TYPE_VENDOR: u8 = 0x0;
/// `VPD_DEVID_TYPE_T10`.
pub const VPD_DEVID_TYPE_T10: u8 = 0x1;
/// `VPD_DEVID_TYPE_EUI64`.
pub const VPD_DEVID_TYPE_EUI64: u8 = 0x2;
/// `VPD_DEVID_TYPE_NAA`.
pub const VPD_DEVID_TYPE_NAA: u8 = 0x3;
/// `VPD_DEVID_TYPE_RELATIVE`.
pub const VPD_DEVID_TYPE_RELATIVE: u8 = 0x4;
/// `VPD_DEVID_TYPE_PORT`.
pub const VPD_DEVID_TYPE_PORT: u8 = 0x5;
/// `VPD_DEVID_TYPE_LU`.
pub const VPD_DEVID_TYPE_LU: u8 = 0x6;
/// `VPD_DEVID_TYPE_MD5`.
pub const VPD_DEVID_TYPE_MD5: u8 = 0x7;
/// `VPD_DEVID_TYPE_NAME`.
pub const VPD_DEVID_TYPE_NAME: u8 = 0x8;

/// `VPD_ATA_COMMAND_CODE_ATA` (`scsi_vpd_ata.command_code`).
pub const VPD_ATA_COMMAND_CODE_ATA: u8 = 0xec;
/// `VPD_ATA_COMMAND_CODE_ATAPI`.
pub const VPD_ATA_COMMAND_CODE_ATAPI: u8 = 0xa1;

/// `RC16_PROT_EN` (`scsi_read_cap_data_16.p_type_prot`): protection type is 0 when 0.
pub const RC16_PROT_EN: u8 = 0x01;
/// `RC16_PROT_P_TYPE`.
pub const RC16_PROT_P_TYPE: u8 = 0x0e;
/// `RC16_P_TYPE_1`: protection type 1.
pub const RC16_P_TYPE_1: u8 = 0x00;
/// `RC16_P_TYPE_2`: protection type 2.
pub const RC16_P_TYPE_2: u8 = 0x02;
/// `RC16_P_TYPE_3`: protection type 3.
pub const RC16_P_TYPE_3: u8 = 0x04;
/// `RC16_BASIS`: meaning of `addr`.
pub const RC16_BASIS: u8 = 0x30;
/// `RC16_BASIS_HIGH`: highest LBA of zone.
pub const RC16_BASIS_HIGH: u8 = 0x00;
/// `RC16_BASIS_LAST`: last LBA on unit.
pub const RC16_BASIS_LAST: u8 = 0x10;
/// `RC16_LBPPB_EXPONENT` (`logical_per_phys`): 2**N logical blocks per physical block, 0 means
/// unknown.
pub const RC16_LBPPB_EXPONENT: u8 = 0x0f;
/// `RC16_PIIPLB_EXPONENT`: 2**N protection information intervals per logical block.
pub const RC16_PIIPLB_EXPONENT: u8 = 0xf0;
/// `RC16_LALBA` (`lowest_aligned`): lowest aligned LBA.
pub const RC16_LALBA: u16 = 0x3fff;
/// `RC16_LBPRZ`: an unmapped LBA returns all zeros.
pub const RC16_LBPRZ: u16 = 0x4000;
/// `READ_CAP_16_TPRZ`: old name of [`RC16_LBPRZ`] used in drivers.
pub const READ_CAP_16_TPRZ: u16 = 0x4000;
/// `RC16_LBPME`: logical block provisioning management enabled.
pub const RC16_LBPME: u16 = 0x8000;
/// `READ_CAP_16_TPE`: old name of [`RC16_LBPME`] used in drivers.
pub const READ_CAP_16_TPE: u16 = 0x8000;

/// `SSD_ERRCODE_CURRENT` (`scsi_sense_data.error_code`).
pub const SSD_ERRCODE_CURRENT: u8 = 0x70;
/// `SSD_ERRCODE_DEFERRED`.
pub const SSD_ERRCODE_DEFERRED: u8 = 0x71;
/// `SSD_ERRCODE`.
pub const SSD_ERRCODE: u8 = 0x7F;
/// `SSD_ERRCODE_VALID`.
pub const SSD_ERRCODE_VALID: u8 = 0x80;
/// `SSD_KEY` (`flags`): the sense key.
pub const SSD_KEY: u8 = 0x0F;
/// `SSD_ILI`: incorrect length indicator.
pub const SSD_ILI: u8 = 0x20;
/// `SSD_EOM`: end of medium.
pub const SSD_EOM: u8 = 0x40;
/// `SSD_FILEMARK`.
pub const SSD_FILEMARK: u8 = 0x80;
/// `SSD_SCS_VALID` (`sense_key_spec_1`).
pub const SSD_SCS_VALID: u8 = 0x80;
/// `SSD_SCS_CDB_ERROR`.
pub const SSD_SCS_CDB_ERROR: u8 = 0x40;
/// `SSD_SCS_SEGMENT_DESC`.
pub const SSD_SCS_SEGMENT_DESC: u8 = 0x20;
/// `SSD_SCS_VALID_BIT_INDEX`.
pub const SSD_SCS_VALID_BIT_INDEX: u8 = 0x08;
/// `SSD_SCS_BIT_INDEX`.
pub const SSD_SCS_BIT_INDEX: u8 = 0x07;

/// `SKEY_NO_SENSE`.
pub const SKEY_NO_SENSE: u8 = 0x00;
/// `SKEY_RECOVERED_ERROR`.
pub const SKEY_RECOVERED_ERROR: u8 = 0x01;
/// `SKEY_NOT_READY`.
pub const SKEY_NOT_READY: u8 = 0x02;
/// `SKEY_MEDIUM_ERROR`.
pub const SKEY_MEDIUM_ERROR: u8 = 0x03;
/// `SKEY_HARDWARE_ERROR`.
pub const SKEY_HARDWARE_ERROR: u8 = 0x04;
/// `SKEY_ILLEGAL_REQUEST`.
pub const SKEY_ILLEGAL_REQUEST: u8 = 0x05;
/// `SKEY_UNIT_ATTENTION`.
pub const SKEY_UNIT_ATTENTION: u8 = 0x06;
/// `SKEY_WRITE_PROTECT`.
pub const SKEY_WRITE_PROTECT: u8 = 0x07;
/// `SKEY_BLANK_CHECK`.
pub const SKEY_BLANK_CHECK: u8 = 0x08;
/// `SKEY_VENDOR_UNIQUE`.
pub const SKEY_VENDOR_UNIQUE: u8 = 0x09;
/// `SKEY_COPY_ABORTED`.
pub const SKEY_COPY_ABORTED: u8 = 0x0A;
/// `SKEY_ABORTED_COMMAND`.
pub const SKEY_ABORTED_COMMAND: u8 = 0x0B;
/// `SKEY_EQUAL`.
pub const SKEY_EQUAL: u8 = 0x0C;
/// `SKEY_VOLUME_OVERFLOW`.
pub const SKEY_VOLUME_OVERFLOW: u8 = 0x0D;
/// `SKEY_MISCOMPARE`.
pub const SKEY_MISCOMPARE: u8 = 0x0E;
/// `SKEY_RESERVED`.
pub const SKEY_RESERVED: u8 = 0x0F;

/* Additional sense code info: the values of ASC_ASCQ(). */
/// `SENSE_FILEMARK_DETECTED`.
pub const SENSE_FILEMARK_DETECTED: u16 = 0x0001;
/// `SENSE_END_OF_MEDIUM_DETECTED`.
pub const SENSE_END_OF_MEDIUM_DETECTED: u16 = 0x0002;
/// `SENSE_SETMARK_DETECTED`.
pub const SENSE_SETMARK_DETECTED: u16 = 0x0003;
/// `SENSE_BEGINNING_OF_MEDIUM_DETECTED`.
pub const SENSE_BEGINNING_OF_MEDIUM_DETECTED: u16 = 0x0004;
/// `SENSE_END_OF_DATA_DETECTED`.
pub const SENSE_END_OF_DATA_DETECTED: u16 = 0x0005;
/// `SENSE_NOT_READY_BECOMING_READY`.
pub const SENSE_NOT_READY_BECOMING_READY: u16 = 0x0401;
/// `SENSE_NOT_READY_INIT_REQUIRED`.
pub const SENSE_NOT_READY_INIT_REQUIRED: u16 = 0x0402;
/// `SENSE_NOT_READY_FORMAT`.
pub const SENSE_NOT_READY_FORMAT: u16 = 0x0404;
/// `SENSE_NOT_READY_REBUILD`.
pub const SENSE_NOT_READY_REBUILD: u16 = 0x0405;
/// `SENSE_NOT_READY_RECALC`.
pub const SENSE_NOT_READY_RECALC: u16 = 0x0406;
/// `SENSE_NOT_READY_INPROGRESS`.
pub const SENSE_NOT_READY_INPROGRESS: u16 = 0x0407;
/// `SENSE_NOT_READY_LONGWRITE`.
pub const SENSE_NOT_READY_LONGWRITE: u16 = 0x0408;
/// `SENSE_NOT_READY_SELFTEST`.
pub const SENSE_NOT_READY_SELFTEST: u16 = 0x0409;
/// `SENSE_POWER_RESET_OR_BUS`.
pub const SENSE_POWER_RESET_OR_BUS: u16 = 0x2900;
/// `SENSE_POWER_ON`.
pub const SENSE_POWER_ON: u16 = 0x2901;
/// `SENSE_BUS_RESET`.
pub const SENSE_BUS_RESET: u16 = 0x2902;
/// `SENSE_BUS_DEVICE_RESET`.
pub const SENSE_BUS_DEVICE_RESET: u16 = 0x2903;
/// `SENSE_DEVICE_INTERNAL_RESET`.
pub const SENSE_DEVICE_INTERNAL_RESET: u16 = 0x2904;
/// `SENSE_TSC_CHANGE_SE`.
pub const SENSE_TSC_CHANGE_SE: u16 = 0x2905;
/// `SENSE_TSC_CHANGE_LVD`.
pub const SENSE_TSC_CHANGE_LVD: u16 = 0x2906;
/// `SENSE_IT_NEXUS_LOSS`.
pub const SENSE_IT_NEXUS_LOSS: u16 = 0x2907;
/// `SENSE_BAD_MEDIUM`.
pub const SENSE_BAD_MEDIUM: u16 = 0x3000;
/// `SENSE_NR_MEDIUM_UNKNOWN_FORMAT`.
pub const SENSE_NR_MEDIUM_UNKNOWN_FORMAT: u16 = 0x3001;
/// `SENSE_NR_MEDIUM_INCOMPATIBLE_FORMAT`.
pub const SENSE_NR_MEDIUM_INCOMPATIBLE_FORMAT: u16 = 0x3002;
/// `SENSE_NW_MEDIUM_UNKNOWN_FORMAT`.
pub const SENSE_NW_MEDIUM_UNKNOWN_FORMAT: u16 = 0x3004;
/// `SENSE_NW_MEDIUM_INCOMPATIBLE_FORMAT`.
pub const SENSE_NW_MEDIUM_INCOMPATIBLE_FORMAT: u16 = 0x3005;
/// `SENSE_NF_MEDIUM_INCOMPATIBLE_FORMAT`.
pub const SENSE_NF_MEDIUM_INCOMPATIBLE_FORMAT: u16 = 0x3006;
/// `SENSE_NW_MEDIUM_AC_MISMATCH`.
pub const SENSE_NW_MEDIUM_AC_MISMATCH: u16 = 0x3008;
/// `SENSE_NOMEDIUM`.
pub const SENSE_NOMEDIUM: u16 = 0x3A00;
/// `SENSE_NOMEDIUM_TCLOSED`.
pub const SENSE_NOMEDIUM_TCLOSED: u16 = 0x3A01;
/// `SENSE_NOMEDIUM_TOPEN`.
pub const SENSE_NOMEDIUM_TOPEN: u16 = 0x3A02;
/// `SENSE_NOMEDIUM_LOADABLE`.
pub const SENSE_NOMEDIUM_LOADABLE: u16 = 0x3A03;
/// `SENSE_NOMEDIUM_AUXMEM`.
pub const SENSE_NOMEDIUM_AUXMEM: u16 = 0x3A04;
/// `SENSE_CARTRIDGE_FAULT`.
pub const SENSE_CARTRIDGE_FAULT: u16 = 0x5200;
/// `SENSE_MEDIUM_REMOVAL_PREVENTED`.
pub const SENSE_MEDIUM_REMOVAL_PREVENTED: u16 = 0x5302;

/// `LONGLBA` (`scsi_mode_header_big.reserved`).
pub const LONGLBA: u8 = 0x01;

/// `SMH_DSP_WRITE_PROT`: both disks and tapes use `dev_spec` to report READONLY status.
pub const SMH_DSP_WRITE_PROT: u8 = 0x80;

/// `RPL_LUNDATA_SIZE`: bytes per LUN in the REPORT LUNS data.
pub const RPL_LUNDATA_SIZE: usize = 8;
/// `RPL_LUNDATA_T0LUN`: a type 0 LUN is in `lundata[1]`.
pub const RPL_LUNDATA_T0LUN: usize = 1;

/*
 * ATA PASS-THROUGH as per SAT2
 */
/// `ATA_PASSTHRU_12`.
pub const ATA_PASSTHRU_12: u8 = 0xa1;
/// `ATA_PASSTHRU_16`.
pub const ATA_PASSTHRU_16: u8 = 0x85;

/// `ATA_PASSTHRU_PROTO_MASK`.
pub const ATA_PASSTHRU_PROTO_MASK: u8 = 0x1e;
/// `ATA_PASSTHRU_PROTO_HW_RESET`.
pub const ATA_PASSTHRU_PROTO_HW_RESET: u8 = 0x00;
/// `ATA_PASSTHRU_PROTO_SW_RESET`.
pub const ATA_PASSTHRU_PROTO_SW_RESET: u8 = 0x02;
/// `ATA_PASSTHRU_PROTO_NON_DATA`.
pub const ATA_PASSTHRU_PROTO_NON_DATA: u8 = 0x06;
/// `ATA_PASSTHRU_PROTO_PIO_DATAIN`.
pub const ATA_PASSTHRU_PROTO_PIO_DATAIN: u8 = 0x08;
/// `ATA_PASSTHRU_PROTO_PIO_DATAOUT`.
pub const ATA_PASSTHRU_PROTO_PIO_DATAOUT: u8 = 0x0a;
/// `ATA_PASSTHRU_PROTO_DMA`.
pub const ATA_PASSTHRU_PROTO_DMA: u8 = 0x0c;
/// `ATA_PASSTHRU_PROTO_DMA_QUEUED`.
pub const ATA_PASSTHRU_PROTO_DMA_QUEUED: u8 = 0x0e;
/// `ATA_PASSTHRU_PROTO_EXEC_DIAG`.
pub const ATA_PASSTHRU_PROTO_EXEC_DIAG: u8 = 0x10;
/// `ATA_PASSTHRU_PROTO_NON_DATA_RST`.
pub const ATA_PASSTHRU_PROTO_NON_DATA_RST: u8 = 0x12;
/// `ATA_PASSTHRU_PROTO_UDMA_DATAIN`.
pub const ATA_PASSTHRU_PROTO_UDMA_DATAIN: u8 = 0x14;
/// `ATA_PASSTHRU_PROTO_UDMA_DATAOUT`.
pub const ATA_PASSTHRU_PROTO_UDMA_DATAOUT: u8 = 0x16;
/// `ATA_PASSTHRU_PROTO_FPDMA`.
pub const ATA_PASSTHRU_PROTO_FPDMA: u8 = 0x18;
/// `ATA_PASSTHRU_PROTO_RESPONSE`.
pub const ATA_PASSTHRU_PROTO_RESPONSE: u8 = 0x1e;

/// `ATA_PASSTHRU_T_DIR_MASK`.
pub const ATA_PASSTHRU_T_DIR_MASK: u8 = 0x08;
/// `ATA_PASSTHRU_T_DIR_READ`.
pub const ATA_PASSTHRU_T_DIR_READ: u8 = 0x08;
/// `ATA_PASSTHRU_T_DIR_WRITE`.
pub const ATA_PASSTHRU_T_DIR_WRITE: u8 = 0x00;

/// `ATA_PASSTHRU_T_LEN_MASK`.
pub const ATA_PASSTHRU_T_LEN_MASK: u8 = 0x03;
/// `ATA_PASSTHRU_T_LEN_NONE`.
pub const ATA_PASSTHRU_T_LEN_NONE: u8 = 0x00;
/// `ATA_PASSTHRU_T_LEN_FEATURES`.
pub const ATA_PASSTHRU_T_LEN_FEATURES: u8 = 0x01;
/// `ATA_PASSTHRU_T_LEN_SECTOR_COUNT`.
pub const ATA_PASSTHRU_T_LEN_SECTOR_COUNT: u8 = 0x02;
/// `ATA_PASSTHRU_T_LEN_TPSIU`.
pub const ATA_PASSTHRU_T_LEN_TPSIU: u8 = 0x03;

/// `SIU_SNSVALID` (`scsi_status_iu_header.flags`).
pub const SIU_SNSVALID: u8 = 0x2;
/// `SIU_RSPVALID`.
pub const SIU_RSPVALID: u8 = 0x1;

/// `SIU_PFC_NONE` (the packet failure code).
pub const SIU_PFC_NONE: u8 = 0x00;
/// `SIU_PFC_CIU_FIELDS_INVALID`.
pub const SIU_PFC_CIU_FIELDS_INVALID: u8 = 0x02;
/// `SIU_PFC_TMF_NOT_SUPPORTED`.
pub const SIU_PFC_TMF_NOT_SUPPORTED: u8 = 0x04;
/// `SIU_PFC_TMF_FAILED`.
pub const SIU_PFC_TMF_FAILED: u8 = 0x05;
/// `SIU_PFC_INVALID_TYPE_CODE`.
pub const SIU_PFC_INVALID_TYPE_CODE: u8 = 0x06;
/// `SIU_PFC_ILLEGAL_REQUEST`.
pub const SIU_PFC_ILLEGAL_REQUEST: u8 = 0x07;

/*
 * Values for 'Task Management Flags' field of SPI command information unit.
 * See section 14.3.1 of SPI-3.
 */
/// `SIU_TASKMGMT_NONE`.
pub const SIU_TASKMGMT_NONE: u8 = 0x00;
/// `SIU_TASKMGMT_ABORT_TASK`.
pub const SIU_TASKMGMT_ABORT_TASK: u8 = 0x01;
/// `SIU_TASKMGMT_ABORT_TASK_SET`.
pub const SIU_TASKMGMT_ABORT_TASK_SET: u8 = 0x02;
/// `SIU_TASKMGMT_CLEAR_TASK_SET`.
pub const SIU_TASKMGMT_CLEAR_TASK_SET: u8 = 0x04;
/// `SIU_TASKMGMT_LUN_RESET`.
pub const SIU_TASKMGMT_LUN_RESET: u8 = 0x08;
/// `SIU_TASKMGMT_TARGET_RESET`.
pub const SIU_TASKMGMT_TARGET_RESET: u8 = 0x20;
/// `SIU_TASKMGMT_CLEAR_ACA`.
pub const SIU_TASKMGMT_CLEAR_ACA: u8 = 0x40;

/*
 * Status Byte
 */
/// `SCSI_OK`.
pub const SCSI_OK: u8 = 0x00;
/// `SCSI_CHECK`: check condition; the sense data says why.
pub const SCSI_CHECK: u8 = 0x02;
/// `SCSI_COND_MET`.
pub const SCSI_COND_MET: u8 = 0x04;
/// `SCSI_BUSY`.
pub const SCSI_BUSY: u8 = 0x08;
/// `SCSI_INTERM`.
pub const SCSI_INTERM: u8 = 0x10;
/// `SCSI_INTERM_COND_MET`.
pub const SCSI_INTERM_COND_MET: u8 = 0x14;
/// `SCSI_RESV_CONFLICT`.
pub const SCSI_RESV_CONFLICT: u8 = 0x18;
/// `SCSI_TERMINATED`.
pub const SCSI_TERMINATED: u8 = 0x22;
/// `SCSI_QUEUE_FULL`: old (pre SCSI-3) name.
pub const SCSI_QUEUE_FULL: u8 = 0x28;
/// `SCSI_TASKSET_FULL`: new (SCSI-3) name.
pub const SCSI_TASKSET_FULL: u8 = 0x28;
/// `SCSI_ACA_ACTIVE`.
pub const SCSI_ACA_ACTIVE: u8 = 0x30;

/// `struct scsi_generic`: a command descriptor block of up to 16 bytes, the opcode first.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScsiGeneric {
    /// `opcode`.
    pub opcode: u8,
    /// `bytes`: the rest of the CDB.
    pub bytes: [u8; 15],
}

/// `struct scsi_test_unit_ready`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiTestUnitReady {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `unused`.
    pub unused: [u8; 3],
    /// `control`.
    pub control: u8,
}

/// `struct scsi_send_diag`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiSendDiag {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`: `SSD_UOL`, `SSD_DOL`, `SSD_SELFTEST`, `SSD_PF`.
    pub byte2: u8,
    /// `unused`.
    pub unused: [u8; 1],
    /// `paramlen`.
    pub paramlen: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `struct scsi_sense`: REQUEST SENSE.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiSense {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `unused`.
    pub unused: [u8; 2],
    /// `length`.
    pub length: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_inquiry`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiInquiry {
    /// `opcode`.
    pub opcode: u8,
    /// `flags`: `SI_EVPD`.
    pub flags: u8,
    /// `pagecode`: `SI_PG_*`.
    pub pagecode: u8,
    /// `length`: big-endian allocation length.
    pub length: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `struct scsi_mode_sense`: MODE SENSE (6).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiModeSense {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`: `SMS_DBD`.
    pub byte2: u8,
    /// `page`: `SMS_PAGE_CODE` and `SMS_PAGE_CTRL` bits.
    pub page: u8,
    /// `unused`.
    pub unused: u8,
    /// `length`.
    pub length: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_mode_sense_big`: MODE SENSE (10).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiModeSenseBig {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`: the bits of the small version plus `SMS_LLBAA`.
    pub byte2: u8,
    /// `page`: the bits of the small version.
    pub page: u8,
    /// `unused`.
    pub unused: [u8; 4],
    /// `length`.
    pub length: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `struct scsi_mode_select`: MODE SELECT (6).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiModeSelect {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`: `SMS_SP`, `SMS_PF`.
    pub byte2: u8,
    /// `unused`.
    pub unused: [u8; 2],
    /// `length`.
    pub length: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_mode_select_big`: MODE SELECT (10).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiModeSelectBig {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`: the bits of the small version.
    pub byte2: u8,
    /// `unused`.
    pub unused: [u8; 5],
    /// `length`.
    pub length: [u8; 2],
    /// `control`.
    pub control: u8,
}

/// `struct scsi_reserve`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReserve {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `unused`.
    pub unused: [u8; 2],
    /// `length`.
    pub length: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_release`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiRelease {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `unused`.
    pub unused: [u8; 2],
    /// `length`.
    pub length: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_prevent`: PREVENT ALLOW MEDIUM REMOVAL.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiPrevent {
    /// `opcode`.
    pub opcode: u8,
    /// `byte2`.
    pub byte2: u8,
    /// `unused`.
    pub unused: [u8; 2],
    /// `how`: `PR_PREVENT` or `PR_ALLOW`.
    pub how: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_report_luns`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReportLuns {
    /// `opcode`.
    pub opcode: u8,
    /// `unused`.
    pub unused: u8,
    /// `selectreport`: `REPORT_*`.
    pub selectreport: u8,
    /// `unused2`.
    pub unused2: [u8; 3],
    /// `length`.
    pub length: [u8; 4],
    /// `unused4`.
    pub unused4: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_inquiry_data`: the standard INQUIRY data.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScsiInquiryData {
    /// `device`: `SID_TYPE` and `SID_QUAL`.
    pub device: u8,
    /// `dev_qual2`: `SID_QUAL2`, `SID_REMOVABLE`.
    pub dev_qual2: u8,
    /// `version`: `SID_ANSII`, `SID_ECMA`, `SID_ISO`.
    pub version: u8,
    /// `response_format`: `SID_RESPONSE_DATA_FMT` and flags.
    pub response_format: u8,
    /// `additional_length`.
    pub additional_length: u8,
    /// `spc3_flags`: `SPC3_SID_*`.
    pub spc3_flags: u8,
    /// `spc2_flags`: `SPC2_*`.
    pub spc2_flags: u8,
    /// `flags`: `SID_VS`, `SID_CmdQue`, ...
    pub flags: u8,
    /// `vendor`.
    pub vendor: [u8; 8],
    /// `product`.
    pub product: [u8; 16],
    /// `revision`.
    pub revision: [u8; 4],
    /// `extra`.
    pub extra: [u8; 20],
    /// `reserved`.
    pub reserved: [u8; 2],
    /// `version_descriptor0`.
    pub version_descriptor0: [u8; 2],
    /// `version_descriptor1`.
    pub version_descriptor1: [u8; 2],
    /// `version_descriptor2`.
    pub version_descriptor2: [u8; 2],
    /// `version_descriptor3`.
    pub version_descriptor3: [u8; 2],
    /// `version_descriptor4`.
    pub version_descriptor4: [u8; 2],
    /// `version_descriptor5`.
    pub version_descriptor5: [u8; 2],
    /// `version_descriptor6`.
    pub version_descriptor6: [u8; 2],
    /// `version_descriptor7`.
    pub version_descriptor7: [u8; 2],
    /// `reserved2`.
    pub reserved2: [u8; 22],
}

impl ScsiInquiryData {
    /// All zeros, as `memset(&link->inqdata, 0, sizeof(link->inqdata))` leaves it.
    pub const fn new() -> Self {
        Self {
            device: 0,
            dev_qual2: 0,
            version: 0,
            response_format: 0,
            additional_length: 0,
            spc3_flags: 0,
            spc2_flags: 0,
            flags: 0,
            vendor: [0; 8],
            product: [0; 16],
            revision: [0; 4],
            extra: [0; 20],
            reserved: [0; 2],
            version_descriptor0: [0; 2],
            version_descriptor1: [0; 2],
            version_descriptor2: [0; 2],
            version_descriptor3: [0; 2],
            version_descriptor4: [0; 2],
            version_descriptor5: [0; 2],
            version_descriptor6: [0; 2],
            version_descriptor7: [0; 2],
            reserved2: [0; 22],
        }
    }
}

impl Default for ScsiInquiryData {
    fn default() -> Self {
        Self::new()
    }
}

/// `struct scsi_vpd_hdr`: the header of every vital product data page.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiVpdHdr {
    /// `device`.
    pub device: u8,
    /// `page_code`.
    pub page_code: u8,
    /// `page_length`.
    pub page_length: [u8; 2],
}

/// `struct scsi_vpd_serial`: the unit serial number page.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiVpdSerial {
    /// `hdr`.
    pub hdr: ScsiVpdHdr,
    /// `serial`.
    pub serial: [u8; 32],
}

/// `struct scsi_vpd_devid_hdr`: a designation descriptor's header in the device
/// identification page.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiVpdDevidHdr {
    /// `pi_code`: [`vpd_devid_pi`] and [`vpd_devid_code`].
    pub pi_code: u8,
    /// `flags`: `VPD_DEVID_PIV`, [`vpd_devid_assoc`], [`vpd_devid_type`].
    pub flags: u8,
    /// `reserved`.
    pub reserved: u8,
    /// `len`.
    pub len: u8,
}

/// `struct scsi_vpd_ata`: the ATA information page.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ScsiVpdAta {
    /// `hdr`.
    pub hdr: ScsiVpdHdr,
    /// `_reserved1`.
    pub _reserved1: [u8; 4],
    /// `sat_vendor`.
    pub sat_vendor: [u8; 8],
    /// `sat_product`.
    pub sat_product: [u8; 16],
    /// `sat_revision`.
    pub sat_revision: [u8; 4],
    /// `device_signature`.
    pub device_signature: [u8; 20],
    /// `command_code`: `VPD_ATA_COMMAND_CODE_*`.
    pub command_code: u8,
    /// `_reserved2`.
    pub _reserved2: [u8; 3],
    /// `identify`.
    pub identify: [u8; 512],
}

/// `struct scsi_read_cap_data`: the READ CAPACITY (10) data.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReadCapData {
    /// `addr`: the last logical block address.
    pub addr: [u8; 4],
    /// `length`: the block length in bytes.
    pub length: [u8; 4],
}

/// `struct scsi_read_cap_data_16`: the READ CAPACITY (16) data.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReadCapData16 {
    /// `addr`: the last logical block address.
    pub addr: [u8; 8],
    /// `length`: the block length in bytes.
    pub length: [u8; 4],
    /// `p_type_prot`: `RC16_PROT_EN`, `RC16_PROT_P_TYPE`, `RC16_BASIS`.
    pub p_type_prot: u8,
    /// `logical_per_phys`: logical blocks per physical block exponent.
    pub logical_per_phys: u8,
    /// `lowest_aligned`: `RC16_LALBA`, `RC16_LBPRZ`, `RC16_LBPME`.
    pub lowest_aligned: [u8; 2],
    /// `reserved`.
    pub reserved: [u8; 16],
}

/// `struct scsi_sense_data_unextended`: the old non-extended sense data.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiSenseDataUnextended {
    /// `error_code`.
    pub error_code: u8,
    /// `block`.
    pub block: [u8; 3],
}

/// `struct scsi_sense_data`: the fixed format sense data (18 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScsiSenseData {
    /// `error_code`: `SSD_ERRCODE*`.
    pub error_code: u8,
    /// `segment`.
    pub segment: u8,
    /// `flags`: `SSD_KEY`, `SSD_ILI`, `SSD_EOM`, `SSD_FILEMARK`.
    pub flags: u8,
    /// `info`.
    pub info: [u8; 4],
    /// `extra_len`.
    pub extra_len: u8,
    /// `cmd_spec_info`.
    pub cmd_spec_info: [u8; 4],
    /// `add_sense_code`: the ASC.
    pub add_sense_code: u8,
    /// `add_sense_code_qual`: the ASCQ.
    pub add_sense_code_qual: u8,
    /// `fru`.
    pub fru: u8,
    /// `sense_key_spec_1`: `SSD_SCS_*`.
    pub sense_key_spec_1: u8,
    /// `sense_key_spec_2`.
    pub sense_key_spec_2: u8,
    /// `sense_key_spec_3`.
    pub sense_key_spec_3: u8,
}

impl ScsiSenseData {
    /// All zeros.
    pub const fn new() -> Self {
        Self {
            error_code: 0,
            segment: 0,
            flags: 0,
            info: [0; 4],
            extra_len: 0,
            cmd_spec_info: [0; 4],
            add_sense_code: 0,
            add_sense_code_qual: 0,
            fru: 0,
            sense_key_spec_1: 0,
            sense_key_spec_2: 0,
            sense_key_spec_3: 0,
        }
    }
}

/// `struct scsi_blk_desc`: a general mode parameter block descriptor.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiBlkDesc {
    /// `density`.
    pub density: u8,
    /// `nblocks`.
    pub nblocks: [u8; 3],
    /// `reserved`.
    pub reserved: u8,
    /// `blklen`.
    pub blklen: [u8; 3],
}

/// `struct scsi_direct_blk_desc`: the direct access block descriptor.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiDirectBlkDesc {
    /// `nblocks`.
    pub nblocks: [u8; 4],
    /// `density`.
    pub density: u8,
    /// `blklen`.
    pub blklen: [u8; 3],
}

/// `struct scsi_blk_desc_big`: the long LBA block descriptor.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiBlkDescBig {
    /// `nblocks`.
    pub nblocks: [u8; 8],
    /// `density`.
    pub density: u8,
    /// `reserved`.
    pub reserved: [u8; 3],
    /// `blklen`.
    pub blklen: [u8; 4],
}

/// `struct scsi_mode_header`: the MODE SENSE (6) / MODE SELECT (6) parameter header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiModeHeader {
    /// `data_length`: sense data length.
    pub data_length: u8,
    /// `medium_type`.
    pub medium_type: u8,
    /// `dev_spec`: `SMH_DSP_WRITE_PROT`.
    pub dev_spec: u8,
    /// `blk_desc_len`.
    pub blk_desc_len: u8,
}

/// `struct scsi_mode_header_big`: the MODE SENSE (10) / MODE SELECT (10) parameter header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiModeHeaderBig {
    /// `data_length`: sense data length.
    pub data_length: [u8; 2],
    /// `medium_type`.
    pub medium_type: u8,
    /// `dev_spec`: `SMH_DSP_WRITE_PROT`.
    pub dev_spec: u8,
    /// `reserved`: `LONGLBA`.
    pub reserved: u8,
    /// `reserved2`.
    pub reserved2: u8,
    /// `blk_desc_len`.
    pub blk_desc_len: [u8; 2],
}

/// `union scsi_mode_sense_buf`: a mode sense reply, 254 bytes (255 and 256 break some
/// devices: ahci does not like 255, various do not like 256 because the length must fit in 8
/// bits). See the module's deviations.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ScsiModeSenseBuf {
    /// `buf`: the bytes.
    pub buf: [u8; 254],
}

impl ScsiModeSenseBuf {
    /// A zeroed buffer.
    pub const fn new() -> Self {
        Self { buf: [0; 254] }
    }

    /// `hdr`: the MODE SENSE (6) header at the front.
    pub fn hdr(&self) -> &ScsiModeHeader {
        wire_ref(&self.buf)
    }

    /// `hdr`, writable.
    pub fn hdr_mut(&mut self) -> &mut ScsiModeHeader {
        wire_mut(&mut self.buf)
    }

    /// `hdr_big`: the MODE SENSE (10) header at the front.
    pub fn hdr_big(&self) -> &ScsiModeHeaderBig {
        wire_ref(&self.buf)
    }

    /// `hdr_big`, writable.
    pub fn hdr_big_mut(&mut self) -> &mut ScsiModeHeaderBig {
        wire_mut(&mut self.buf)
    }
}

impl Default for ScsiModeSenseBuf {
    fn default() -> Self {
        Self::new()
    }
}

/// One entry of `scsi_report_luns_data.luns`: the anonymous `struct { u_int8_t
/// lundata[RPL_LUNDATA_SIZE]; }`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiReportLunsLun {
    /// `lundata`: only the type zero form is supported for now.
    pub lundata: [u8; RPL_LUNDATA_SIZE],
}

/// `struct scsi_report_luns_data`: the REPORT LUNS reply.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ScsiReportLunsData {
    /// `length`: length of the LUN inventory, in bytes.
    pub length: [u8; 4],
    /// `reserved`: unused.
    pub reserved: [u8; 4],
    /// `luns`: the LUN inventory (`scsi_link->luns` is `u_int8_t`, hence 256).
    pub luns: [ScsiReportLunsLun; 256],
}

/// `struct scsi_lun_array`: the LUNs a target reports, for `scsi_probe_target`.
#[derive(Clone, Copy, Debug)]
pub struct ScsiLunArray {
    /// `luns`.
    pub luns: [u8; 256],
    /// `count`.
    pub count: i32,
    /// `dumbscan`.
    pub dumbscan: i32,
}

/// `struct scsi_ata_passthru_12`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiAtaPassthru12 {
    /// `opcode`.
    pub opcode: u8,
    /// `count_proto`.
    pub count_proto: u8,
    /// `flags`.
    pub flags: u8,
    /// `features`.
    pub features: u8,
    /// `sector_count`.
    pub sector_count: u8,
    /// `lba_low`.
    pub lba_low: u8,
    /// `lba_mid`.
    pub lba_mid: u8,
    /// `lba_high`.
    pub lba_high: u8,
    /// `device`.
    pub device: u8,
    /// `command`.
    pub command: u8,
    /// `_reserved`.
    pub _reserved: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_ata_passthru_16`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiAtaPassthru16 {
    /// `opcode`.
    pub opcode: u8,
    /// `count_proto`.
    pub count_proto: u8,
    /// `flags`.
    pub flags: u8,
    /// `features`.
    pub features: [u8; 2],
    /// `sector_count`.
    pub sector_count: [u8; 2],
    /// `lba_low`.
    pub lba_low: [u8; 2],
    /// `lba_mid`.
    pub lba_mid: [u8; 2],
    /// `lba_high`.
    pub lba_high: [u8; 2],
    /// `device`.
    pub device: u8,
    /// `command`.
    pub command: u8,
    /// `control`.
    pub control: u8,
}

/// `struct scsi_status_iu_header`: the SPI status information unit (section 14.3.5 of SPI-3).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ScsiStatusIuHeader {
    /// `reserved`.
    pub reserved: [u8; 2],
    /// `flags`: `SIU_SNSVALID`, `SIU_RSPVALID`.
    pub flags: u8,
    /// `status`.
    pub status: u8,
    /// `sense_length`.
    pub sense_length: [u8; 4],
    /// `pkt_failures_length`.
    pub pkt_failures_length: [u8; 4],
    /// `data`: the packet failure list and the sense data, or the sense data; the first byte
    /// of a variable-length tail.
    pub data: [u8; 1],
}

/// A SCSI wire structure: a command descriptor block or a data format.
///
/// # Safety
///
/// Implement only for `#[repr(C)]` types made of `u8`s and arrays and structures of them:
/// the alignment is 1, there is no padding, and every bit pattern is a valid value.
pub unsafe trait ScsiWire: Copy + 'static {
    /// A value of all-zero bytes (`memset(&x, 0, sizeof(x))`).
    fn zeroed() -> Self {
        // SAFETY: every bit pattern, all zeros included, is a valid value (the trait's
        // contract).
        unsafe { core::mem::zeroed() }
    }

    /// The structure's bytes (`(u_char *)&x`).
    fn as_bytes(&self) -> &[u8] {
        // SAFETY: no padding, so all `size_of::<Self>()` bytes are initialised; the slice
        // borrows `self`.
        unsafe { slice::from_raw_parts(core::ptr::from_ref(self).cast::<u8>(), size_of::<Self>()) }
    }

    /// The structure's bytes, writable.
    fn as_bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as for `as_bytes`, and any bytes written make a valid value.
        unsafe {
            slice::from_raw_parts_mut(core::ptr::from_mut(self).cast::<u8>(), size_of::<Self>())
        }
    }

    /// A copy of the structure at the front of `bytes`; missing trailing bytes read as 0.
    fn read_from(bytes: &[u8]) -> Self {
        let mut v = Self::zeroed();
        let n = bytes.len().min(size_of::<Self>());
        v.as_bytes_mut()[..n].copy_from_slice(&bytes[..n]);
        v
    }
}

/// Marks wire structures (see [`ScsiWire`]).
macro_rules! scsi_wire {
    ($($t:ty),* $(,)?) => {
        $(
            // SAFETY: `#[repr(C)]` of `u8`s and byte arrays only (the size assertions at the
            // end of the file pin that there is no padding).
            unsafe impl ScsiWire for $t {}
        )*
    };
}

scsi_wire!(
    ScsiGeneric,
    ScsiTestUnitReady,
    ScsiSendDiag,
    ScsiSense,
    ScsiInquiry,
    ScsiModeSense,
    ScsiModeSenseBig,
    ScsiModeSelect,
    ScsiModeSelectBig,
    ScsiReserve,
    ScsiRelease,
    ScsiPrevent,
    ScsiReportLuns,
    ScsiInquiryData,
    ScsiVpdHdr,
    ScsiVpdSerial,
    ScsiVpdDevidHdr,
    ScsiVpdAta,
    ScsiReadCapData,
    ScsiReadCapData16,
    ScsiSenseDataUnextended,
    ScsiSenseData,
    ScsiBlkDesc,
    ScsiDirectBlkDesc,
    ScsiBlkDescBig,
    ScsiModeHeader,
    ScsiModeHeaderBig,
    ScsiModeSenseBuf,
    ScsiReportLunsLun,
    ScsiReportLunsData,
    ScsiAtaPassthru12,
    ScsiAtaPassthru16,
    ScsiStatusIuHeader,
);

/// `(struct T *)bytes`: the wire structure at the front of `bytes`.
///
/// Panics (an index out of range) when `bytes` is shorter than a `T`.
pub fn wire_ref<T: ScsiWire>(bytes: &[u8]) -> &T {
    let bytes = &bytes[..size_of::<T>()];
    // SAFETY: `T: ScsiWire` has alignment 1 and accepts any bytes; the slice covers the whole
    // `T` and the reference borrows it.
    unsafe { &*bytes.as_ptr().cast::<T>() }
}

/// `(struct T *)bytes`, writable.
///
/// Panics (an index out of range) when `bytes` is shorter than a `T`.
pub fn wire_mut<T: ScsiWire>(bytes: &mut [u8]) -> &mut T {
    let bytes = &mut bytes[..size_of::<T>()];
    // SAFETY: as for `wire_ref`; any value written leaves valid bytes behind.
    unsafe { &mut *bytes.as_mut_ptr().cast::<T>() }
}

/// `VPD_DEVID_PI(_f)`: the protocol identifier of a designation descriptor.
pub const fn vpd_devid_pi(f: u8) -> u8 {
    (f >> 4) & 0x0f
}

/// `VPD_DEVID_CODE(_f)`: the code set.
pub const fn vpd_devid_code(f: u8) -> u8 {
    f & 0x0f
}

/// `VPD_DEVID_ASSOC(_f)`: the association.
pub const fn vpd_devid_assoc(f: u8) -> u8 {
    f & 0x30
}

/// `VPD_DEVID_TYPE(_f)`: the designator type.
pub const fn vpd_devid_type(f: u8) -> u8 {
    f & 0x0f
}

/// `ASC_ASCQ(ssd)`: the additional sense code and its qualifier as one number (the
/// `SENSE_*` values).
pub const fn asc_ascq(ssd: &ScsiSenseData) -> u16 {
    ((ssd.add_sense_code as u16) << 8) | ssd.add_sense_code_qual as u16
}

/// `VALID_MODE_HDR(_x)`: whether a MODE SENSE (6) reply has at least a header.
pub const fn valid_mode_hdr(x: &ScsiModeHeader) -> bool {
    x.data_length >= 3
}

/// `VALID_MODE_HDR_BIG(_x)`: whether a MODE SENSE (10) reply has at least a header.
pub fn valid_mode_hdr_big(x: &ScsiModeHeaderBig) -> bool {
    _2btol(&x.data_length) >= 6
}

/// `SIU_PKTFAIL_CODE(siu)`: the packet failure code of the status information unit `iu`
/// (the whole unit, header first).
pub fn siu_pktfail_code(iu: &[u8]) -> u8 {
    iu[core::mem::offset_of!(ScsiStatusIuHeader, data) + 3]
}

/// `SIU_SENSE_LENGTH(siu)`.
pub fn siu_sense_length(siu: &ScsiStatusIuHeader) -> u32 {
    _4btol(&siu.sense_length)
}

/// `SIU_SENSE_DATA(siu)`, as the offset of the sense data from `siu->data`: past the packet
/// failure list when `SIU_RSPVALID` is set, else at the start.
pub fn siu_sense_data_offset(siu: &ScsiStatusIuHeader) -> usize {
    if siu.flags & SIU_RSPVALID != 0 {
        _4btol(&siu.pkt_failures_length) as usize
    } else {
        0
    }
}

const _: () = {
    assert!(size_of::<ScsiGeneric>() == 16);
    assert!(size_of::<ScsiTestUnitReady>() == 6);
    assert!(size_of::<ScsiSendDiag>() == 6);
    assert!(size_of::<ScsiSense>() == 6);
    assert!(size_of::<ScsiInquiry>() == 6);
    assert!(size_of::<ScsiModeSense>() == 6);
    assert!(size_of::<ScsiModeSenseBig>() == 10);
    assert!(size_of::<ScsiModeSelect>() == 6);
    assert!(size_of::<ScsiModeSelectBig>() == 10);
    assert!(size_of::<ScsiReserve>() == 6);
    assert!(size_of::<ScsiRelease>() == 6);
    assert!(size_of::<ScsiPrevent>() == 6);
    assert!(size_of::<ScsiReportLuns>() == 12);
    assert!(size_of::<ScsiInquiryData>() == 96);
    assert!(size_of::<ScsiVpdHdr>() == 4);
    assert!(size_of::<ScsiVpdSerial>() == 36);
    assert!(size_of::<ScsiVpdDevidHdr>() == 4);
    assert!(size_of::<ScsiVpdAta>() == 572);
    assert!(size_of::<ScsiReadCapData>() == 8);
    assert!(size_of::<ScsiReadCapData16>() == 32);
    assert!(size_of::<ScsiSenseDataUnextended>() == 4);
    assert!(size_of::<ScsiSenseData>() == 18);
    assert!(size_of::<ScsiBlkDesc>() == 8);
    assert!(size_of::<ScsiDirectBlkDesc>() == 8);
    assert!(size_of::<ScsiBlkDescBig>() == 16);
    assert!(size_of::<ScsiModeHeader>() == 4);
    assert!(size_of::<ScsiModeHeaderBig>() == 8);
    assert!(size_of::<ScsiModeSenseBuf>() == 254);
    assert!(size_of::<ScsiReportLunsData>() == 8 + 256 * RPL_LUNDATA_SIZE);
    assert!(size_of::<ScsiAtaPassthru12>() == 12);
    assert!(size_of::<ScsiAtaPassthru16>() == 16);
    assert!(size_of::<ScsiStatusIuHeader>() == 13);
    assert!(core::mem::align_of::<ScsiInquiryData>() == 1);
    assert!(core::mem::align_of::<ScsiReportLunsData>() == 1);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for `scsi_all.rs`.

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
}
/* </TESTS> */
