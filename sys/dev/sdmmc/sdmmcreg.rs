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

/*
 * Copyright (c) 2006 Uwe Stuehler <uwe@openbsd.org>
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
//! `<dev/sdmmc/sdmmcreg.h>`: the SD and MMC command set, the OCR bits, the R1 status bits and
//! the layout of the CSD, CID, SCR and switch-function status registers.
//!
//! Upstream: sys/dev/sdmmc/sdmmcreg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are functions with the macros' names in lower case
//!   (`MMC_CSD_CSDVER(resp)` is [`mmc_csd_csdver`]), as `PCI_VENDOR` is `pci_vendor`. The
//!   `*_PNM_*_CPY` macros take the destination array by reference.
//! - `__bitfield` reads the response words as the bytes they are in memory
//!   (`u32::to_ne_bytes`), as the C's `u_int8_t *` walk does; a bit range past the end of the
//!   words reads as zero bits where the C would read past the array.
//! - The constants take the type of the field or argument they go into: command indices are
//!   `u16` (`c_opcode`), OCR and R1 bits `u32`, `EXT_CSD` byte offsets and values `u8`, and
//!   what is compared with a decoded register field `i32` (what `__bitfield` returns).

/* MMC commands */

/// `MMC_GO_IDLE_STATE` (R0).
pub const MMC_GO_IDLE_STATE: u16 = 0;
/// `MMC_SEND_OP_COND` (R3).
pub const MMC_SEND_OP_COND: u16 = 1;
/// `MMC_ALL_SEND_CID` (R2).
pub const MMC_ALL_SEND_CID: u16 = 2;
/// `MMC_SET_RELATIVE_ADDR` (R1).
pub const MMC_SET_RELATIVE_ADDR: u16 = 3;
/// `MMC_SWITCH` (R1B).
pub const MMC_SWITCH: u16 = 6;
/// `MMC_SELECT_CARD` (R1).
pub const MMC_SELECT_CARD: u16 = 7;
/// `MMC_SEND_EXT_CSD` (R1).
pub const MMC_SEND_EXT_CSD: u16 = 8;
/// `MMC_SEND_CSD` (R2).
pub const MMC_SEND_CSD: u16 = 9;
/// `MMC_STOP_TRANSMISSION` (R1B).
pub const MMC_STOP_TRANSMISSION: u16 = 12;
/// `MMC_SEND_STATUS` (R1).
pub const MMC_SEND_STATUS: u16 = 13;
/// `MMC_SET_BLOCKLEN` (R1).
pub const MMC_SET_BLOCKLEN: u16 = 16;
/// `MMC_READ_BLOCK_SINGLE` (R1).
pub const MMC_READ_BLOCK_SINGLE: u16 = 17;
/// `MMC_READ_BLOCK_MULTIPLE` (R1).
pub const MMC_READ_BLOCK_MULTIPLE: u16 = 18;
/// `MMC_SEND_TUNING_BLOCK` (R1).
pub const MMC_SEND_TUNING_BLOCK: u16 = 19;
/// `MMC_SEND_TUNING_BLOCK_HS200` (R1).
pub const MMC_SEND_TUNING_BLOCK_HS200: u16 = 21;
/// `MMC_SET_BLOCK_COUNT` (R1).
pub const MMC_SET_BLOCK_COUNT: u16 = 23;
/// `MMC_WRITE_BLOCK_SINGLE` (R1).
pub const MMC_WRITE_BLOCK_SINGLE: u16 = 24;
/// `MMC_WRITE_BLOCK_MULTIPLE` (R1).
pub const MMC_WRITE_BLOCK_MULTIPLE: u16 = 25;
/// `MMC_APP_CMD` (R1).
pub const MMC_APP_CMD: u16 = 55;

/* SD commands */

/// `SD_SEND_RELATIVE_ADDR` (R6).
pub const SD_SEND_RELATIVE_ADDR: u16 = 3;
/// `SD_SEND_SWITCH_FUNC` (R1).
pub const SD_SEND_SWITCH_FUNC: u16 = 6;
/// `SD_SEND_IF_COND` (R7).
pub const SD_SEND_IF_COND: u16 = 8;
/// `SD_VOLTAGE_SWITCH` (R1).
pub const SD_VOLTAGE_SWITCH: u16 = 11;

/* SD application commands */

/// `SD_APP_SET_BUS_WIDTH` (R1).
pub const SD_APP_SET_BUS_WIDTH: u16 = 6;
/// `SD_APP_OP_COND` (R3).
pub const SD_APP_OP_COND: u16 = 41;
/// `SD_APP_SEND_SCR` (R1).
pub const SD_APP_SEND_SCR: u16 = 51;

/* OCR bits */

/// `MMC_OCR_MEM_READY`: memory power-up status bit.
pub const MMC_OCR_MEM_READY: u32 = 1 << 31;
/// `MMC_OCR_HCS`: SD only.
pub const MMC_OCR_HCS: u32 = 1 << 30;
/// `MMC_OCR_ACCESS_MODE_MASK`: MMC only.
pub const MMC_OCR_ACCESS_MODE_MASK: u32 = 3 << 29;
/// `MMC_OCR_ACCESS_MODE_BYTE`: MMC only.
pub const MMC_OCR_ACCESS_MODE_BYTE: u32 = 0 << 29;
/// `MMC_OCR_ACCESS_MODE_SECTOR`: MMC only.
pub const MMC_OCR_ACCESS_MODE_SECTOR: u32 = 2 << 29;
/// `MMC_OCR_S18A`.
pub const MMC_OCR_S18A: u32 = 1 << 24;
/// `MMC_OCR_3_5V_3_6V`.
pub const MMC_OCR_3_5V_3_6V: u32 = 1 << 23;
/// `MMC_OCR_3_4V_3_5V`.
pub const MMC_OCR_3_4V_3_5V: u32 = 1 << 22;
/// `MMC_OCR_3_3V_3_4V`.
pub const MMC_OCR_3_3V_3_4V: u32 = 1 << 21;
/// `MMC_OCR_3_2V_3_3V`.
pub const MMC_OCR_3_2V_3_3V: u32 = 1 << 20;
/// `MMC_OCR_3_1V_3_2V`.
pub const MMC_OCR_3_1V_3_2V: u32 = 1 << 19;
/// `MMC_OCR_3_0V_3_1V`.
pub const MMC_OCR_3_0V_3_1V: u32 = 1 << 18;
/// `MMC_OCR_2_9V_3_0V`.
pub const MMC_OCR_2_9V_3_0V: u32 = 1 << 17;
/// `MMC_OCR_2_8V_2_9V`.
pub const MMC_OCR_2_8V_2_9V: u32 = 1 << 16;
/// `MMC_OCR_2_7V_2_8V`.
pub const MMC_OCR_2_7V_2_8V: u32 = 1 << 15;
/// `MMC_OCR_2_6V_2_7V`.
pub const MMC_OCR_2_6V_2_7V: u32 = 1 << 14;
/// `MMC_OCR_2_5V_2_6V`.
pub const MMC_OCR_2_5V_2_6V: u32 = 1 << 13;
/// `MMC_OCR_2_4V_2_5V`.
pub const MMC_OCR_2_4V_2_5V: u32 = 1 << 12;
/// `MMC_OCR_2_3V_2_4V`.
pub const MMC_OCR_2_3V_2_4V: u32 = 1 << 11;
/// `MMC_OCR_2_2V_2_3V`.
pub const MMC_OCR_2_2V_2_3V: u32 = 1 << 10;
/// `MMC_OCR_2_1V_2_2V`.
pub const MMC_OCR_2_1V_2_2V: u32 = 1 << 9;
/// `MMC_OCR_2_0V_2_1V`.
pub const MMC_OCR_2_0V_2_1V: u32 = 1 << 8;
/// `MMC_OCR_1_65V_1_95V`.
pub const MMC_OCR_1_65V_1_95V: u32 = 1 << 7;

/// `SD_OCR_VOL_MASK`: bits 23:15.
pub const SD_OCR_VOL_MASK: u32 = 0xFF8000;

/* R1 response type bits */

/// `MMC_R1_READY_FOR_DATA`: ready for next transfer.
pub const MMC_R1_READY_FOR_DATA: u32 = 1 << 8;
/// `MMC_R1_APP_CMD`: app. commands supported.
pub const MMC_R1_APP_CMD: u32 = 1 << 5;

/* bus width argument */

/// `SD_ARG_BUS_WIDTH_1`.
pub const SD_ARG_BUS_WIDTH_1: u32 = 0;
/// `SD_ARG_BUS_WIDTH_4`.
pub const SD_ARG_BUS_WIDTH_4: u32 = 2;

/* EXT_CSD fields */

/// `EXT_CSD_BUS_WIDTH` (WO).
pub const EXT_CSD_BUS_WIDTH: u8 = 183;
/// `EXT_CSD_HS_TIMING` (R/W).
pub const EXT_CSD_HS_TIMING: u8 = 185;
/// `EXT_CSD_REV` (RO).
pub const EXT_CSD_REV: u8 = 192;
/// `EXT_CSD_STRUCTURE` (RO).
pub const EXT_CSD_STRUCTURE: u8 = 194;
/// `EXT_CSD_CARD_TYPE` (RO).
pub const EXT_CSD_CARD_TYPE: u8 = 196;
/// `EXT_CSD_SEC_COUNT` (RO).
pub const EXT_CSD_SEC_COUNT: u8 = 212;

/* EXT_CSD field definitions */

/// `EXT_CSD_CMD_SET_NORMAL`.
pub const EXT_CSD_CMD_SET_NORMAL: u8 = 1 << 0;
/// `EXT_CSD_CMD_SET_SECURE`.
pub const EXT_CSD_CMD_SET_SECURE: u8 = 1 << 1;
/// `EXT_CSD_CMD_SET_CPSECURE`.
pub const EXT_CSD_CMD_SET_CPSECURE: u8 = 1 << 2;

/* EXT_CSD_HS_TIMING */

/// `EXT_CSD_HS_TIMING_BC`.
pub const EXT_CSD_HS_TIMING_BC: u8 = 0;
/// `EXT_CSD_HS_TIMING_HS`.
pub const EXT_CSD_HS_TIMING_HS: u8 = 1;
/// `EXT_CSD_HS_TIMING_HS200`.
pub const EXT_CSD_HS_TIMING_HS200: u8 = 2;
/// `EXT_CSD_HS_TIMING_HS400`.
pub const EXT_CSD_HS_TIMING_HS400: u8 = 3;

/* EXT_CSD_BUS_WIDTH */

/// `EXT_CSD_BUS_WIDTH_1`.
pub const EXT_CSD_BUS_WIDTH_1: u8 = 0;
/// `EXT_CSD_BUS_WIDTH_4`.
pub const EXT_CSD_BUS_WIDTH_4: u8 = 1;
/// `EXT_CSD_BUS_WIDTH_8`.
pub const EXT_CSD_BUS_WIDTH_8: u8 = 2;
/// `EXT_CSD_BUS_WIDTH_4_DDR`.
pub const EXT_CSD_BUS_WIDTH_4_DDR: u8 = 5;
/// `EXT_CSD_BUS_WIDTH_8_DDR`.
pub const EXT_CSD_BUS_WIDTH_8_DDR: u8 = 6;

/* EXT_CSD_CARD_TYPE */
/* The only currently valid values for this field are 0x01, 0x03, 0x07,
 * 0x0B and 0x0F. */

/// `EXT_CSD_CARD_TYPE_F_26M`.
pub const EXT_CSD_CARD_TYPE_F_26M: u8 = 1 << 0;
/// `EXT_CSD_CARD_TYPE_F_52M`.
pub const EXT_CSD_CARD_TYPE_F_52M: u8 = 1 << 1;
/// `EXT_CSD_CARD_TYPE_F_DDR52_1_8V`.
pub const EXT_CSD_CARD_TYPE_F_DDR52_1_8V: u8 = 1 << 2;
/// `EXT_CSD_CARD_TYPE_F_DDR52_1_2V`.
pub const EXT_CSD_CARD_TYPE_F_DDR52_1_2V: u8 = 1 << 3;
/// `EXT_CSD_CARD_TYPE_F_HS200_1_8V`.
pub const EXT_CSD_CARD_TYPE_F_HS200_1_8V: u8 = 1 << 4;
/// `EXT_CSD_CARD_TYPE_F_HS200_1_2V`.
pub const EXT_CSD_CARD_TYPE_F_HS200_1_2V: u8 = 1 << 5;
/// `EXT_CSD_CARD_TYPE_F_HS400_1_8V`.
pub const EXT_CSD_CARD_TYPE_F_HS400_1_8V: u8 = 1 << 6;
/// `EXT_CSD_CARD_TYPE_F_HS400_1_2V`.
pub const EXT_CSD_CARD_TYPE_F_HS400_1_2V: u8 = 1 << 7;

/* MMC_SWITCH access mode */

/// `MMC_SWITCH_MODE_CMD_SET`: change the command set.
pub const MMC_SWITCH_MODE_CMD_SET: u32 = 0x00;
/// `MMC_SWITCH_MODE_SET_BITS`: set bits in value.
pub const MMC_SWITCH_MODE_SET_BITS: u32 = 0x01;
/// `MMC_SWITCH_MODE_CLEAR_BITS`: clear bits in value.
pub const MMC_SWITCH_MODE_CLEAR_BITS: u32 = 0x02;
/// `MMC_SWITCH_MODE_WRITE_BYTE`: set target to value.
pub const MMC_SWITCH_MODE_WRITE_BYTE: u32 = 0x03;

/* MMC R2 response (CSD) */

/// `MMC_CSD_CSDVER_1_0`.
pub const MMC_CSD_CSDVER_1_0: i32 = 1;
/// `MMC_CSD_CSDVER_2_0`.
pub const MMC_CSD_CSDVER_2_0: i32 = 2;
/// `MMC_CSD_CSDVER_EXT_CSD`.
pub const MMC_CSD_CSDVER_EXT_CSD: i32 = 3;
/// `MMC_CSD_MMCVER_1_0`: MMC 1.0 - 1.2.
pub const MMC_CSD_MMCVER_1_0: i32 = 0;
/// `MMC_CSD_MMCVER_1_4`: MMC 1.4.
pub const MMC_CSD_MMCVER_1_4: i32 = 1;
/// `MMC_CSD_MMCVER_2_0`: MMC 2.0 - 2.2.
pub const MMC_CSD_MMCVER_2_0: i32 = 2;
/// `MMC_CSD_MMCVER_3_1`: MMC 3.1 - 3.3.
pub const MMC_CSD_MMCVER_3_1: i32 = 3;
/// `MMC_CSD_MMCVER_4_0`: MMC 4.
pub const MMC_CSD_MMCVER_4_0: i32 = 4;

/* SD R2 response (CSD) */

/// `SD_CSD_CSDVER_1_0`.
pub const SD_CSD_CSDVER_1_0: i32 = 0;
/// `SD_CSD_CSDVER_2_0`.
pub const SD_CSD_CSDVER_2_0: i32 = 1;
/// `SD_CSD_TAAC_1_5_MSEC`.
pub const SD_CSD_TAAC_1_5_MSEC: i32 = 0x26;
/// `SD_CSD_SPEED_25_MHZ`.
pub const SD_CSD_SPEED_25_MHZ: i32 = 0x32;
/// `SD_CSD_SPEED_50_MHZ`.
pub const SD_CSD_SPEED_50_MHZ: i32 = 0x5a;
/// `SD_CSD_CCC_BASIC`: basic.
pub const SD_CSD_CCC_BASIC: i32 = 1 << 0;
/// `SD_CSD_CCC_BR`: block read.
pub const SD_CSD_CCC_BR: i32 = 1 << 2;
/// `SD_CSD_CCC_BW`: block write.
pub const SD_CSD_CCC_BW: i32 = 1 << 4;
/// `SD_CSD_CCC_ERACE`: erase.
pub const SD_CSD_CCC_ERACE: i32 = 1 << 5;
/// `SD_CSD_CCC_WP`: write protection.
pub const SD_CSD_CCC_WP: i32 = 1 << 6;
/// `SD_CSD_CCC_LC`: lock card.
pub const SD_CSD_CCC_LC: i32 = 1 << 7;
/// `SD_CSD_CCC_AS`: application specific.
pub const SD_CSD_CCC_AS: i32 = 1 << 8;
/// `SD_CSD_CCC_IOM`: I/O mode.
pub const SD_CSD_CCC_IOM: i32 = 1 << 9;
/// `SD_CSD_CCC_SWITCH`: switch.
pub const SD_CSD_CCC_SWITCH: i32 = 1 << 10;
/// `SD_CSD_V2_BL_LEN`: 512.
pub const SD_CSD_V2_BL_LEN: i32 = 0x9;
/// `SD_CSD_VDD_RW_CURR_100mA`.
#[allow(non_upper_case_globals)] // the C's name
pub const SD_CSD_VDD_RW_CURR_100mA: i32 = 0x7;
/// `SD_CSD_VDD_RW_CURR_80mA`.
#[allow(non_upper_case_globals)] // the C's name
pub const SD_CSD_VDD_RW_CURR_80mA: i32 = 0x6;
/// `SD_CSD_RW_BL_LEN_2G`.
pub const SD_CSD_RW_BL_LEN_2G: i32 = 0xa;
/// `SD_CSD_RW_BL_LEN_1G`.
pub const SD_CSD_RW_BL_LEN_1G: i32 = 0x9;

/* SCR (SD Configuration Register) */

/// `SCR_STRUCTURE_VER_1_0`: version 1.0.
pub const SCR_STRUCTURE_VER_1_0: i32 = 0;
/// `SCR_SD_SPEC_VER_1_0`: version 1.0 and 1.01.
pub const SCR_SD_SPEC_VER_1_0: i32 = 0;
/// `SCR_SD_SPEC_VER_1_10`: version 1.10.
pub const SCR_SD_SPEC_VER_1_10: i32 = 1;
/// `SCR_SD_SPEC_VER_2`: version 2.00 or version 3.0X.
pub const SCR_SD_SPEC_VER_2: i32 = 2;
/// `SCR_SD_SECURITY_NONE`: no security.
pub const SCR_SD_SECURITY_NONE: i32 = 0;
/// `SCR_SD_SECURITY_1_0`: security protocol 1.0.
pub const SCR_SD_SECURITY_1_0: i32 = 1;
/// `SCR_SD_SECURITY_1_0_2`: security protocol 1.0.
pub const SCR_SD_SECURITY_1_0_2: i32 = 2;
/// `SCR_SD_BUS_WIDTHS_1BIT`: 1bit (DAT0).
pub const SCR_SD_BUS_WIDTHS_1BIT: i32 = 1 << 0;
/// `SCR_SD_BUS_WIDTHS_4BIT`: 4bit (DAT0-3).
pub const SCR_SD_BUS_WIDTHS_4BIT: i32 = 1 << 2;

/// `SD_ACCESS_MODE_SDR12`.
pub const SD_ACCESS_MODE_SDR12: i32 = 0;
/// `SD_ACCESS_MODE_SDR25`.
pub const SD_ACCESS_MODE_SDR25: i32 = 1;
/// `SD_ACCESS_MODE_SDR50`.
pub const SD_ACCESS_MODE_SDR50: i32 = 2;
/// `SD_ACCESS_MODE_SDR104`.
pub const SD_ACCESS_MODE_SDR104: i32 = 3;
/// `SD_ACCESS_MODE_DDR50`.
pub const SD_ACCESS_MODE_DDR50: i32 = 4;

/* 48-bit response decoding (32 bits w/o CRC) */

/// `MMC_R1(resp)`.
pub const fn mmc_r1(resp: &[u32]) -> u32 {
    resp[0]
}

/// `MMC_R3(resp)`.
pub const fn mmc_r3(resp: &[u32]) -> u32 {
    resp[0]
}

/// `SD_R6(resp)`.
pub const fn sd_r6(resp: &[u32]) -> u32 {
    resp[0]
}

/* RCA argument and response */

/// `MMC_ARG_RCA(rca)`.
pub const fn mmc_arg_rca(rca: u16) -> u32 {
    (rca as u32) << 16
}

/// `SD_R6_RCA(resp)`.
pub const fn sd_r6_rca(resp: &[u32]) -> u16 {
    (sd_r6(resp) >> 16) as u16
}

/* MMC R2 response (CSD) */

/// `MMC_CSD_CSDVER(resp)`.
pub fn mmc_csd_csdver(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 126, 2)
}

/// `MMC_CSD_MMCVER(resp)`.
pub fn mmc_csd_mmcver(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 122, 4)
}

/// `MMC_CSD_READ_BL_LEN(resp)`.
pub fn mmc_csd_read_bl_len(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 80, 4)
}

/// `MMC_CSD_C_SIZE(resp)`.
pub fn mmc_csd_c_size(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 62, 12)
}

/// `MMC_CSD_CAPACITY(resp)`.
pub fn mmc_csd_capacity(resp: &[u32]) -> i32 {
    (mmc_csd_c_size(resp) + 1) << (mmc_csd_c_size_mult(resp) + 2)
}

/// `MMC_CSD_C_SIZE_MULT(resp)`.
pub fn mmc_csd_c_size_mult(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 47, 3)
}

/* MMC v1 R2 response (CID) */

/// `MMC_CID_MID_V1(resp)`.
pub fn mmc_cid_mid_v1(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 104, 24)
}

/// `MMC_CID_PNM_V1_CPY(resp, pnm)`: the seven name bytes and a NUL.
pub fn mmc_cid_pnm_v1_cpy(resp: &[u32], pnm: &mut [u8; 8]) {
    for (i, start) in [96, 88, 80, 72, 64, 56, 48].into_iter().enumerate() {
        pnm[i] = mmc_rsp_bits(resp, start, 8) as u8;
    }
    pnm[7] = 0;
}

/// `MMC_CID_REV_V1(resp)`.
pub fn mmc_cid_rev_v1(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 40, 8)
}

/// `MMC_CID_PSN_V1(resp)`.
pub fn mmc_cid_psn_v1(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 16, 24)
}

/// `MMC_CID_MDT_V1(resp)`.
pub fn mmc_cid_mdt_v1(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 8, 8)
}

/* MMC v2 R2 response (CID) */

/// `MMC_CID_MID_V2(resp)`.
pub fn mmc_cid_mid_v2(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 120, 8)
}

/// `MMC_CID_OID_V2(resp)`.
pub fn mmc_cid_oid_v2(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 104, 16)
}

/// `MMC_CID_PNM_V2_CPY(resp, pnm)`: the six name bytes and a NUL.
pub fn mmc_cid_pnm_v2_cpy(resp: &[u32], pnm: &mut [u8; 8]) {
    for (i, start) in [96, 88, 80, 72, 64, 56].into_iter().enumerate() {
        pnm[i] = mmc_rsp_bits(resp, start, 8) as u8;
    }
    pnm[6] = 0;
}

/// `MMC_CID_PSN_V2(resp)`.
pub fn mmc_cid_psn_v2(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 16, 32)
}

/* SD R2 response (CSD) */

/// `SD_CSD_CSDVER(resp)`.
pub fn sd_csd_csdver(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 126, 2)
}

/// `SD_CSD_TAAC(resp)`.
pub fn sd_csd_taac(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 112, 8)
}

/// `SD_CSD_NSAC(resp)`.
pub fn sd_csd_nsac(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 104, 8)
}

/// `SD_CSD_SPEED(resp)`.
pub fn sd_csd_speed(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 96, 8)
}

/// `SD_CSD_CCC(resp)`.
pub fn sd_csd_ccc(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 84, 12)
}

/// `SD_CSD_READ_BL_LEN(resp)`.
pub fn sd_csd_read_bl_len(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 80, 4)
}

/// `SD_CSD_READ_BL_PARTIAL(resp)`.
pub fn sd_csd_read_bl_partial(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 79, 1)
}

/// `SD_CSD_WRITE_BLK_MISALIGN(resp)`.
pub fn sd_csd_write_blk_misalign(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 78, 1)
}

/// `SD_CSD_READ_BLK_MISALIGN(resp)`.
pub fn sd_csd_read_blk_misalign(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 77, 1)
}

/// `SD_CSD_DSR_IMP(resp)`.
pub fn sd_csd_dsr_imp(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 76, 1)
}

/// `SD_CSD_C_SIZE(resp)`.
pub fn sd_csd_c_size(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 62, 12)
}

/// `SD_CSD_CAPACITY(resp)`.
pub fn sd_csd_capacity(resp: &[u32]) -> i32 {
    (sd_csd_c_size(resp) + 1) << (sd_csd_c_size_mult(resp) + 2)
}

/// `SD_CSD_V2_C_SIZE(resp)`.
pub fn sd_csd_v2_c_size(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 48, 22)
}

/// `SD_CSD_V2_CAPACITY(resp)`.
pub fn sd_csd_v2_capacity(resp: &[u32]) -> i32 {
    (sd_csd_v2_c_size(resp) + 1) << 10
}

/// `SD_CSD_VDD_R_CURR_MIN(resp)`.
pub fn sd_csd_vdd_r_curr_min(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 59, 3)
}

/// `SD_CSD_VDD_R_CURR_MAX(resp)`.
pub fn sd_csd_vdd_r_curr_max(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 56, 3)
}

/// `SD_CSD_VDD_W_CURR_MIN(resp)`.
pub fn sd_csd_vdd_w_curr_min(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 53, 3)
}

/// `SD_CSD_VDD_W_CURR_MAX(resp)`.
pub fn sd_csd_vdd_w_curr_max(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 50, 3)
}

/// `SD_CSD_C_SIZE_MULT(resp)`.
pub fn sd_csd_c_size_mult(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 47, 3)
}

/// `SD_CSD_ERASE_BLK_EN(resp)`.
pub fn sd_csd_erase_blk_en(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 46, 1)
}

/// `SD_CSD_SECTOR_SIZE(resp)` (+1).
pub fn sd_csd_sector_size(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 39, 7)
}

/// `SD_CSD_WP_GRP_SIZE(resp)` (+1).
pub fn sd_csd_wp_grp_size(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 32, 7)
}

/// `SD_CSD_WP_GRP_ENABLE(resp)`.
pub fn sd_csd_wp_grp_enable(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 31, 1)
}

/// `SD_CSD_R2W_FACTOR(resp)`.
pub fn sd_csd_r2w_factor(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 26, 3)
}

/// `SD_CSD_WRITE_BL_LEN(resp)`.
pub fn sd_csd_write_bl_len(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 22, 4)
}

/// `SD_CSD_WRITE_BL_PARTIAL(resp)`.
pub fn sd_csd_write_bl_partial(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 21, 1)
}

/// `SD_CSD_FILE_FORMAT_GRP(resp)`.
pub fn sd_csd_file_format_grp(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 15, 1)
}

/// `SD_CSD_COPY(resp)`.
pub fn sd_csd_copy(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 14, 1)
}

/// `SD_CSD_PERM_WRITE_PROTECT(resp)`.
pub fn sd_csd_perm_write_protect(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 13, 1)
}

/// `SD_CSD_TMP_WRITE_PROTECT(resp)`.
pub fn sd_csd_tmp_write_protect(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 12, 1)
}

/// `SD_CSD_FILE_FORMAT(resp)`.
pub fn sd_csd_file_format(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 10, 2)
}

/* SD R2 response (CID) */

/// `SD_CID_MID(resp)`.
pub fn sd_cid_mid(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 120, 8)
}

/// `SD_CID_OID(resp)`.
pub fn sd_cid_oid(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 104, 16)
}

/// `SD_CID_PNM_CPY(resp, pnm)`: the five name bytes and a NUL.
pub fn sd_cid_pnm_cpy(resp: &[u32], pnm: &mut [u8; 8]) {
    for (i, start) in [96, 88, 80, 72, 64].into_iter().enumerate() {
        pnm[i] = mmc_rsp_bits(resp, start, 8) as u8;
    }
    pnm[5] = 0;
}

/// `SD_CID_REV(resp)`.
pub fn sd_cid_rev(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 56, 8)
}

/// `SD_CID_PSN(resp)`.
pub fn sd_cid_psn(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 24, 32)
}

/// `SD_CID_MDT(resp)`.
pub fn sd_cid_mdt(resp: &[u32]) -> i32 {
    mmc_rsp_bits(resp, 8, 12)
}

/* SCR (SD Configuration Register) */

/// `SCR_STRUCTURE(scr)`.
pub fn scr_structure(scr: &[u32]) -> i32 {
    mmc_rsp_bits(scr, 60, 4)
}

/// `SCR_SD_SPEC(scr)`.
pub fn scr_sd_spec(scr: &[u32]) -> i32 {
    mmc_rsp_bits(scr, 56, 4)
}

/// `SCR_DATA_STAT_AFTER_ERASE(scr)`.
pub fn scr_data_stat_after_erase(scr: &[u32]) -> i32 {
    mmc_rsp_bits(scr, 55, 1)
}

/// `SCR_SD_SECURITY(scr)`.
pub fn scr_sd_security(scr: &[u32]) -> i32 {
    mmc_rsp_bits(scr, 52, 3)
}

/// `SCR_SD_BUS_WIDTHS(scr)`.
pub fn scr_sd_bus_widths(scr: &[u32]) -> i32 {
    mmc_rsp_bits(scr, 48, 4)
}

/// `SCR_SD_SPEC3(scr)`.
pub fn scr_sd_spec3(scr: &[u32]) -> i32 {
    mmc_rsp_bits(scr, 47, 1)
}

/// `SCR_EX_SECURITY(scr)`.
pub fn scr_ex_security(scr: &[u32]) -> i32 {
    mmc_rsp_bits(scr, 43, 4)
}

/// `SCR_SD_SPEC4(scr)`.
pub fn scr_sd_spec4(scr: &[u32]) -> i32 {
    mmc_rsp_bits(scr, 42, 1)
}

/// `SCR_RESERVED(scr)`.
pub fn scr_reserved(scr: &[u32]) -> i32 {
    mmc_rsp_bits(scr, 34, 8)
}

/// `SCR_CMD_SUPPORT_CMD23(scr)`.
pub fn scr_cmd_support_cmd23(scr: &[u32]) -> i32 {
    mmc_rsp_bits(scr, 33, 1)
}

/// `SCR_CMD_SUPPORT_CMD20(scr)`.
pub fn scr_cmd_support_cmd20(scr: &[u32]) -> i32 {
    mmc_rsp_bits(scr, 32, 1)
}

/// `SCR_RESERVED2(scr)`.
pub fn scr_reserved2(scr: &[u32]) -> i32 {
    mmc_rsp_bits(scr, 0, 32)
}

/* Status of Switch Function */

/// `SFUNC_STATUS_GROUP(status, group)`: the 16 bits of function group `group` in the 512-bit
/// switch function status (made `__bitfield`-compatible by `sdmmc_be512_to_bitfield512`).
pub fn sfunc_status_group(status: &[u32], group: i32) -> i32 {
    __bitfield(status, 400 + (group - 1) * 16, 16)
}

/// `MMC_RSP_BITS(resp, start, len)`: bits `start .. start + len` of a response whose CRC
/// byte the controller stripped (bit 8 of the register is bit 0 of the words).
pub fn mmc_rsp_bits(resp: &[u32], start: i32, len: i32) -> i32 {
    __bitfield(resp, start - 8, len)
}

/// `__bitfield`: `len` bits (at most 32) of the words at `src` from bit `start`, the bits
/// counted from the lowest of the first byte in memory. "Might be slow, but it should work
/// on big and little endian systems."
pub fn __bitfield(src: &[u32], start: i32, len: i32) -> i32 {
    if start < 0 || !(0..=32).contains(&len) {
        return 0;
    }

    let byte = |i: usize| -> u32 {
        src.get(i / 4)
            .map_or(0, |w| u32::from(w.to_ne_bytes()[i % 4]))
    };

    let mut start = start as usize;
    let mut len = len as u32;
    let mut dst: u32 = 0;
    let mask = if !len.is_multiple_of(32) {
        u32::MAX >> (32 - (len % 32))
    } else {
        u32::MAX
    };
    let mut shift = 0u32;

    while len > 0 {
        let bs = (start % 8) as u32;
        let bc = (8 - bs).min(len);
        // `shift` stays below 32 while `len` bits remain (they are at most 32).
        dst |= (byte(start / 8) >> bs).wrapping_shl(shift);
        shift += bc;
        start += bc as usize;
        len -= bc;
    }

    dst &= mask;
    dst as i32
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest;

    /// A response as the controller leaves it: bytes 0..15 of the 136-bit register without
    /// its CRC byte, little-endian words.
    fn resp_from_bytes(bytes: &[u8; 16]) -> [u32; 4] {
        let mut r = [0u32; 4];
        for (i, w) in r.iter_mut().enumerate() {
            *w = u32::from_ne_bytes([
                bytes[i * 4],
                bytes[i * 4 + 1],
                bytes[i * 4 + 2],
                bytes[i * 4 + 3],
            ]);
        }
        r
    }

    #[test]
    fn bitfield_reads_across_bytes_and_words() {
        let w = [0x8765_4321u32.to_le(), 0xfedc_ba98u32.to_le()];
        assert_eq!(__bitfield(&w, 0, 4), 0x1);
        assert_eq!(__bitfield(&w, 4, 8), 0x32);
        assert_eq!(__bitfield(&w, 28, 8), 0x88);
        assert_eq!(__bitfield(&w, 0, 32), 0x8765_4321u32 as i32);
        assert_eq!(__bitfield(&w, 32, 32), 0xfedc_ba98u32 as i32);
        assert_eq!(__bitfield(&w, 60, 4), 0xf);
        // Out-of-range arguments read as 0, as the C's guard does.
        assert_eq!(__bitfield(&w, -1, 4), 0);
        assert_eq!(__bitfield(&w, 0, 33), 0);
        assert_eq!(mmc_rsp_bits(&w, 8, 8), 0x21);
    }

    #[test]
    fn sd_csd_v2_decodes_a_64mb_card() {
        // QEMU's sd.c CSD 2.0 for a 64 MiB card: C_SIZE = (64 MiB / 512 KiB) - 1 = 127.
        // Register bits 127..0 are bytes 15..0; the controller strips bits 7..0 (the CRC),
        // so response byte n holds register bits 8n+15..8n+8.
        let mut reg = [0u8; 17]; // register bytes, index = bit / 8
        reg[15] = 0x40; // CSD_STRUCTURE = 1 (bits 127:126)
        // C_SIZE (bits 69:48) = 127.
        let c_size: u32 = 127;
        for bit in 0..22 {
            if c_size & (1 << bit) != 0 {
                let b = 48 + bit;
                reg[b / 8] |= 1 << (b % 8);
            }
        }
        // CCC (bits 95:84) = 0x5b5.
        let ccc: u32 = 0x5b5;
        for bit in 0..12 {
            if ccc & (1 << bit) != 0 {
                let b = 84 + bit;
                reg[b / 8] |= 1 << (b % 8);
            }
        }
        let mut bytes = [0u8; 16];
        bytes[..15].copy_from_slice(&reg[1..16]);
        let resp = resp_from_bytes(&bytes);
        assert_eq!(sd_csd_csdver(&resp), SD_CSD_CSDVER_2_0);
        assert_eq!(sd_csd_v2_c_size(&resp), 127);
        assert_eq!(sd_csd_v2_capacity(&resp), 131072);
        assert_eq!(sd_csd_ccc(&resp), 0x5b5);
        assert_ne!(sd_csd_ccc(&resp) & SD_CSD_CCC_SWITCH, 0);
    }

    #[test]
    fn sd_cid_name_copies_five_bytes() {
        // QEMU's CID: MID 0xaa, OID "XY", PNM "QEMU!", PRV 0x01.
        let mut reg = [0u8; 17];
        reg[15] = 0xaa;
        reg[14] = b'X';
        reg[13] = b'Y';
        reg[12] = b'Q';
        reg[11] = b'E';
        reg[10] = b'M';
        reg[9] = b'U';
        reg[8] = b'!';
        reg[7] = 0x01;
        let mut bytes = [0u8; 16];
        bytes[..15].copy_from_slice(&reg[1..16]);
        let resp = resp_from_bytes(&bytes);
        let mut pnm = [0xffu8; 8];
        sd_cid_pnm_cpy(&resp, &mut pnm);
        assert_eq!(&pnm[..6], b"QEMU!\0");
        assert_eq!(sd_cid_mid(&resp), 0xaa);
        assert_eq!(sd_cid_oid(&resp), i32::from(u16::from_be_bytes(*b"XY")));
        assert_eq!(sd_cid_rev(&resp), 1);
    }

    #[test]
    fn rca_argument_and_response() {
        assert_eq!(mmc_arg_rca(0x4567), 0x4567_0000);
        assert_eq!(sd_r6_rca(&[0x4567_0120, 0, 0, 0]), 0x4567);
    }

    #[test]
    #[ignore = "reads the C reference (just test-ref)"]
    fn defines_match_the_reference() {
        let defs = reftest::defines("sys/dev/sdmmc/sdmmcreg.h");
        #[allow(clippy::unnecessary_cast)] // a column of mixed types
        let ours: &[(&str, i64)] = &[
            ("MMC_GO_IDLE_STATE", MMC_GO_IDLE_STATE as i64),
            ("MMC_SEND_OP_COND", MMC_SEND_OP_COND as i64),
            ("MMC_ALL_SEND_CID", MMC_ALL_SEND_CID as i64),
            ("MMC_SET_RELATIVE_ADDR", MMC_SET_RELATIVE_ADDR as i64),
            ("MMC_SWITCH", MMC_SWITCH as i64),
            ("MMC_SELECT_CARD", MMC_SELECT_CARD as i64),
            ("MMC_SEND_EXT_CSD", MMC_SEND_EXT_CSD as i64),
            ("MMC_SEND_CSD", MMC_SEND_CSD as i64),
            ("MMC_STOP_TRANSMISSION", MMC_STOP_TRANSMISSION as i64),
            ("MMC_SEND_STATUS", MMC_SEND_STATUS as i64),
            ("MMC_SET_BLOCKLEN", MMC_SET_BLOCKLEN as i64),
            ("MMC_READ_BLOCK_SINGLE", MMC_READ_BLOCK_SINGLE as i64),
            ("MMC_READ_BLOCK_MULTIPLE", MMC_READ_BLOCK_MULTIPLE as i64),
            ("MMC_SEND_TUNING_BLOCK", MMC_SEND_TUNING_BLOCK as i64),
            (
                "MMC_SEND_TUNING_BLOCK_HS200",
                MMC_SEND_TUNING_BLOCK_HS200 as i64,
            ),
            ("MMC_SET_BLOCK_COUNT", MMC_SET_BLOCK_COUNT as i64),
            ("MMC_WRITE_BLOCK_SINGLE", MMC_WRITE_BLOCK_SINGLE as i64),
            ("MMC_WRITE_BLOCK_MULTIPLE", MMC_WRITE_BLOCK_MULTIPLE as i64),
            ("MMC_APP_CMD", MMC_APP_CMD as i64),
            ("SD_SEND_RELATIVE_ADDR", SD_SEND_RELATIVE_ADDR as i64),
            ("SD_SEND_SWITCH_FUNC", SD_SEND_SWITCH_FUNC as i64),
            ("SD_SEND_IF_COND", SD_SEND_IF_COND as i64),
            ("SD_VOLTAGE_SWITCH", SD_VOLTAGE_SWITCH as i64),
            ("SD_APP_SET_BUS_WIDTH", SD_APP_SET_BUS_WIDTH as i64),
            ("SD_APP_OP_COND", SD_APP_OP_COND as i64),
            ("SD_APP_SEND_SCR", SD_APP_SEND_SCR as i64),
            ("MMC_OCR_MEM_READY", MMC_OCR_MEM_READY as i64),
            ("MMC_OCR_HCS", MMC_OCR_HCS as i64),
            ("MMC_OCR_ACCESS_MODE_MASK", MMC_OCR_ACCESS_MODE_MASK as i64),
            ("MMC_OCR_ACCESS_MODE_BYTE", MMC_OCR_ACCESS_MODE_BYTE as i64),
            (
                "MMC_OCR_ACCESS_MODE_SECTOR",
                MMC_OCR_ACCESS_MODE_SECTOR as i64,
            ),
            ("MMC_OCR_S18A", MMC_OCR_S18A as i64),
            ("MMC_OCR_3_5V_3_6V", MMC_OCR_3_5V_3_6V as i64),
            ("MMC_OCR_3_4V_3_5V", MMC_OCR_3_4V_3_5V as i64),
            ("MMC_OCR_3_3V_3_4V", MMC_OCR_3_3V_3_4V as i64),
            ("MMC_OCR_3_2V_3_3V", MMC_OCR_3_2V_3_3V as i64),
            ("MMC_OCR_3_1V_3_2V", MMC_OCR_3_1V_3_2V as i64),
            ("MMC_OCR_3_0V_3_1V", MMC_OCR_3_0V_3_1V as i64),
            ("MMC_OCR_2_9V_3_0V", MMC_OCR_2_9V_3_0V as i64),
            ("MMC_OCR_2_8V_2_9V", MMC_OCR_2_8V_2_9V as i64),
            ("MMC_OCR_2_7V_2_8V", MMC_OCR_2_7V_2_8V as i64),
            ("MMC_OCR_2_6V_2_7V", MMC_OCR_2_6V_2_7V as i64),
            ("MMC_OCR_2_5V_2_6V", MMC_OCR_2_5V_2_6V as i64),
            ("MMC_OCR_2_4V_2_5V", MMC_OCR_2_4V_2_5V as i64),
            ("MMC_OCR_2_3V_2_4V", MMC_OCR_2_3V_2_4V as i64),
            ("MMC_OCR_2_2V_2_3V", MMC_OCR_2_2V_2_3V as i64),
            ("MMC_OCR_2_1V_2_2V", MMC_OCR_2_1V_2_2V as i64),
            ("MMC_OCR_2_0V_2_1V", MMC_OCR_2_0V_2_1V as i64),
            ("MMC_OCR_1_65V_1_95V", MMC_OCR_1_65V_1_95V as i64),
            ("SD_OCR_VOL_MASK", SD_OCR_VOL_MASK as i64),
            ("MMC_R1_READY_FOR_DATA", MMC_R1_READY_FOR_DATA as i64),
            ("MMC_R1_APP_CMD", MMC_R1_APP_CMD as i64),
            ("SD_ARG_BUS_WIDTH_1", SD_ARG_BUS_WIDTH_1 as i64),
            ("SD_ARG_BUS_WIDTH_4", SD_ARG_BUS_WIDTH_4 as i64),
            ("EXT_CSD_BUS_WIDTH", EXT_CSD_BUS_WIDTH as i64),
            ("EXT_CSD_HS_TIMING", EXT_CSD_HS_TIMING as i64),
            ("EXT_CSD_REV", EXT_CSD_REV as i64),
            ("EXT_CSD_STRUCTURE", EXT_CSD_STRUCTURE as i64),
            ("EXT_CSD_CARD_TYPE", EXT_CSD_CARD_TYPE as i64),
            ("EXT_CSD_SEC_COUNT", EXT_CSD_SEC_COUNT as i64),
            ("EXT_CSD_HS_TIMING_BC", EXT_CSD_HS_TIMING_BC as i64),
            ("EXT_CSD_HS_TIMING_HS", EXT_CSD_HS_TIMING_HS as i64),
            ("EXT_CSD_HS_TIMING_HS200", EXT_CSD_HS_TIMING_HS200 as i64),
            ("EXT_CSD_HS_TIMING_HS400", EXT_CSD_HS_TIMING_HS400 as i64),
            ("EXT_CSD_BUS_WIDTH_1", EXT_CSD_BUS_WIDTH_1 as i64),
            ("EXT_CSD_BUS_WIDTH_4", EXT_CSD_BUS_WIDTH_4 as i64),
            ("EXT_CSD_BUS_WIDTH_8", EXT_CSD_BUS_WIDTH_8 as i64),
            ("EXT_CSD_BUS_WIDTH_4_DDR", EXT_CSD_BUS_WIDTH_4_DDR as i64),
            ("EXT_CSD_BUS_WIDTH_8_DDR", EXT_CSD_BUS_WIDTH_8_DDR as i64),
            ("EXT_CSD_CARD_TYPE_F_26M", EXT_CSD_CARD_TYPE_F_26M as i64),
            ("EXT_CSD_CARD_TYPE_F_52M", EXT_CSD_CARD_TYPE_F_52M as i64),
            (
                "EXT_CSD_CARD_TYPE_F_DDR52_1_8V",
                EXT_CSD_CARD_TYPE_F_DDR52_1_8V as i64,
            ),
            (
                "EXT_CSD_CARD_TYPE_F_DDR52_1_2V",
                EXT_CSD_CARD_TYPE_F_DDR52_1_2V as i64,
            ),
            (
                "EXT_CSD_CARD_TYPE_F_HS200_1_8V",
                EXT_CSD_CARD_TYPE_F_HS200_1_8V as i64,
            ),
            (
                "EXT_CSD_CARD_TYPE_F_HS200_1_2V",
                EXT_CSD_CARD_TYPE_F_HS200_1_2V as i64,
            ),
            (
                "EXT_CSD_CARD_TYPE_F_HS400_1_8V",
                EXT_CSD_CARD_TYPE_F_HS400_1_8V as i64,
            ),
            (
                "EXT_CSD_CARD_TYPE_F_HS400_1_2V",
                EXT_CSD_CARD_TYPE_F_HS400_1_2V as i64,
            ),
            ("MMC_SWITCH_MODE_CMD_SET", MMC_SWITCH_MODE_CMD_SET as i64),
            ("MMC_SWITCH_MODE_SET_BITS", MMC_SWITCH_MODE_SET_BITS as i64),
            (
                "MMC_SWITCH_MODE_CLEAR_BITS",
                MMC_SWITCH_MODE_CLEAR_BITS as i64,
            ),
            (
                "MMC_SWITCH_MODE_WRITE_BYTE",
                MMC_SWITCH_MODE_WRITE_BYTE as i64,
            ),
            ("MMC_CSD_CSDVER_1_0", MMC_CSD_CSDVER_1_0 as i64),
            ("MMC_CSD_CSDVER_2_0", MMC_CSD_CSDVER_2_0 as i64),
            ("MMC_CSD_CSDVER_EXT_CSD", MMC_CSD_CSDVER_EXT_CSD as i64),
            ("MMC_CSD_MMCVER_1_0", MMC_CSD_MMCVER_1_0 as i64),
            ("MMC_CSD_MMCVER_1_4", MMC_CSD_MMCVER_1_4 as i64),
            ("MMC_CSD_MMCVER_2_0", MMC_CSD_MMCVER_2_0 as i64),
            ("MMC_CSD_MMCVER_3_1", MMC_CSD_MMCVER_3_1 as i64),
            ("MMC_CSD_MMCVER_4_0", MMC_CSD_MMCVER_4_0 as i64),
            ("SD_CSD_CSDVER_1_0", SD_CSD_CSDVER_1_0 as i64),
            ("SD_CSD_CSDVER_2_0", SD_CSD_CSDVER_2_0 as i64),
            ("SD_CSD_TAAC_1_5_MSEC", SD_CSD_TAAC_1_5_MSEC as i64),
            ("SD_CSD_SPEED_25_MHZ", SD_CSD_SPEED_25_MHZ as i64),
            ("SD_CSD_SPEED_50_MHZ", SD_CSD_SPEED_50_MHZ as i64),
            ("SD_CSD_CCC_BASIC", SD_CSD_CCC_BASIC as i64),
            ("SD_CSD_CCC_BR", SD_CSD_CCC_BR as i64),
            ("SD_CSD_CCC_BW", SD_CSD_CCC_BW as i64),
            ("SD_CSD_CCC_ERACE", SD_CSD_CCC_ERACE as i64),
            ("SD_CSD_CCC_WP", SD_CSD_CCC_WP as i64),
            ("SD_CSD_CCC_LC", SD_CSD_CCC_LC as i64),
            ("SD_CSD_CCC_AS", SD_CSD_CCC_AS as i64),
            ("SD_CSD_CCC_IOM", SD_CSD_CCC_IOM as i64),
            ("SD_CSD_CCC_SWITCH", SD_CSD_CCC_SWITCH as i64),
            ("SD_CSD_V2_BL_LEN", SD_CSD_V2_BL_LEN as i64),
            ("SD_CSD_VDD_RW_CURR_100mA", SD_CSD_VDD_RW_CURR_100mA as i64),
            ("SD_CSD_VDD_RW_CURR_80mA", SD_CSD_VDD_RW_CURR_80mA as i64),
            ("SD_CSD_RW_BL_LEN_2G", SD_CSD_RW_BL_LEN_2G as i64),
            ("SD_CSD_RW_BL_LEN_1G", SD_CSD_RW_BL_LEN_1G as i64),
            ("SCR_STRUCTURE_VER_1_0", SCR_STRUCTURE_VER_1_0 as i64),
            ("SCR_SD_SPEC_VER_1_0", SCR_SD_SPEC_VER_1_0 as i64),
            ("SCR_SD_SPEC_VER_1_10", SCR_SD_SPEC_VER_1_10 as i64),
            ("SCR_SD_SPEC_VER_2", SCR_SD_SPEC_VER_2 as i64),
            ("SCR_SD_SECURITY_NONE", SCR_SD_SECURITY_NONE as i64),
            ("SCR_SD_SECURITY_1_0", SCR_SD_SECURITY_1_0 as i64),
            ("SCR_SD_SECURITY_1_0_2", SCR_SD_SECURITY_1_0_2 as i64),
            ("SCR_SD_BUS_WIDTHS_1BIT", SCR_SD_BUS_WIDTHS_1BIT as i64),
            ("SCR_SD_BUS_WIDTHS_4BIT", SCR_SD_BUS_WIDTHS_4BIT as i64),
            ("SD_ACCESS_MODE_SDR12", SD_ACCESS_MODE_SDR12 as i64),
            ("SD_ACCESS_MODE_SDR25", SD_ACCESS_MODE_SDR25 as i64),
            ("SD_ACCESS_MODE_SDR50", SD_ACCESS_MODE_SDR50 as i64),
            ("SD_ACCESS_MODE_SDR104", SD_ACCESS_MODE_SDR104 as i64),
            ("SD_ACCESS_MODE_DDR50", SD_ACCESS_MODE_DDR50 as i64),
        ];
        for &(name, v) in ours {
            assert_eq!(reftest::int(&defs, name), Some(v), "{name}");
        }
    }
}
/* </TESTS> */
