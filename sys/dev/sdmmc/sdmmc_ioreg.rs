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
//! `<dev/sdmmc/sdmmc_ioreg.h>`: the SDIO commands (CMD5, CMD52, CMD53), their argument
//! fields, the I/O OCR, the Card Common Control Registers, the Function Basic Registers and
//! the Card Information Structure tuple codes.
//!
//! Upstream: sys/dev/sdmmc/sdmmc_ioreg.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - The function-like macros are functions with the macros' names in lower case
//!   (`SD_R5_DATA(resp)` is [`sd_r5_data`]).
//! - Command indices are `u16` (`c_opcode`), argument and OCR bits `u32`, register
//!   addresses `i32` (the C's `int reg`), register values `u8`.

#![allow(clippy::identity_op)] // the constants' `(value << 0)`, kept as the C writes them

/* SDIO commands */

/// `SD_IO_SEND_OP_COND` (R4).
pub const SD_IO_SEND_OP_COND: u16 = 5;
/// `SD_IO_RW_DIRECT` (R5).
pub const SD_IO_RW_DIRECT: u16 = 52;
/// `SD_IO_RW_EXTENDED` (R5?).
pub const SD_IO_RW_EXTENDED: u16 = 53;

/* CMD52 arguments */

/// `SD_ARG_CMD52_READ`.
pub const SD_ARG_CMD52_READ: u32 = 0 << 31;
/// `SD_ARG_CMD52_WRITE`.
pub const SD_ARG_CMD52_WRITE: u32 = 1 << 31;
/// `SD_ARG_CMD52_FUNC_SHIFT`.
pub const SD_ARG_CMD52_FUNC_SHIFT: u32 = 28;
/// `SD_ARG_CMD52_FUNC_MASK`.
pub const SD_ARG_CMD52_FUNC_MASK: u32 = 0x7;
/// `SD_ARG_CMD52_EXCHANGE`.
pub const SD_ARG_CMD52_EXCHANGE: u32 = 1 << 27;
/// `SD_ARG_CMD52_REG_SHIFT`.
pub const SD_ARG_CMD52_REG_SHIFT: u32 = 9;
/// `SD_ARG_CMD52_REG_MASK`.
pub const SD_ARG_CMD52_REG_MASK: u32 = 0x1ffff;
/// `SD_ARG_CMD52_DATA_SHIFT`.
pub const SD_ARG_CMD52_DATA_SHIFT: u32 = 0;
/// `SD_ARG_CMD52_DATA_MASK`.
pub const SD_ARG_CMD52_DATA_MASK: u32 = 0xff;

/* CMD53 arguments */

/// `SD_ARG_CMD53_READ`.
pub const SD_ARG_CMD53_READ: u32 = 0 << 31;
/// `SD_ARG_CMD53_WRITE`.
pub const SD_ARG_CMD53_WRITE: u32 = 1 << 31;
/// `SD_ARG_CMD53_FUNC_SHIFT`.
pub const SD_ARG_CMD53_FUNC_SHIFT: u32 = 28;
/// `SD_ARG_CMD53_FUNC_MASK`.
pub const SD_ARG_CMD53_FUNC_MASK: u32 = 0x7;
/// `SD_ARG_CMD53_BLOCK_MODE`.
pub const SD_ARG_CMD53_BLOCK_MODE: u32 = 1 << 27;
/// `SD_ARG_CMD53_INCREMENT`.
pub const SD_ARG_CMD53_INCREMENT: u32 = 1 << 26;
/// `SD_ARG_CMD53_REG_SHIFT`.
pub const SD_ARG_CMD53_REG_SHIFT: u32 = 9;
/// `SD_ARG_CMD53_REG_MASK`.
pub const SD_ARG_CMD53_REG_MASK: u32 = 0x1ffff;
/// `SD_ARG_CMD53_LENGTH_SHIFT`.
pub const SD_ARG_CMD53_LENGTH_SHIFT: u32 = 0;
/// `SD_ARG_CMD53_LENGTH_MASK`.
pub const SD_ARG_CMD53_LENGTH_MASK: u32 = 0x1ff;
/// `SD_ARG_CMD53_LENGTH_MAX`.
pub const SD_ARG_CMD53_LENGTH_MAX: i32 = 511;

/* SD R4 response (IO OCR) */

/// `SD_IO_OCR_MEM_READY`.
pub const SD_IO_OCR_MEM_READY: u32 = 1 << 31;
/// `SD_IO_OCR_MEM_PRESENT`: XXX big fat memory present "flag" because we don't know better.
pub const SD_IO_OCR_MEM_PRESENT: u32 = 0xf << 24;
/// `SD_IO_OCR_MASK`.
pub const SD_IO_OCR_MASK: u32 = 0x00fffff0;

/* Card Common Control Registers (CCCR) */

/// `SD_IO_CCCR_START`.
pub const SD_IO_CCCR_START: i32 = 0x00000;
/// `SD_IO_CCCR_SIZE`.
pub const SD_IO_CCCR_SIZE: i32 = 0x100;
/// `SD_IO_CCCR_FN_ENABLE`.
pub const SD_IO_CCCR_FN_ENABLE: i32 = 0x02;
/// `SD_IO_CCCR_FN_READY`.
pub const SD_IO_CCCR_FN_READY: i32 = 0x03;
/// `SD_IO_CCCR_INT_ENABLE`.
pub const SD_IO_CCCR_INT_ENABLE: i32 = 0x04;
/// `SD_IO_CCCR_CTL`.
pub const SD_IO_CCCR_CTL: i32 = 0x06;
/// `CCCR_CTL_RES`.
pub const CCCR_CTL_RES: u8 = 1 << 3;
/// `SD_IO_CCCR_BUS_WIDTH`.
pub const SD_IO_CCCR_BUS_WIDTH: i32 = 0x07;
/// `CCCR_BUS_WIDTH_1`.
pub const CCCR_BUS_WIDTH_1: u8 = 0 << 0;
/// `CCCR_BUS_WIDTH_4`.
pub const CCCR_BUS_WIDTH_4: u8 = 2 << 0;
/// `CCCR_BUS_WIDTH_MASK`.
pub const CCCR_BUS_WIDTH_MASK: u8 = 3 << 0;
/// `SD_IO_CCCR_CISPTR`: XXX 9-10, 10-11, or 9-12.
pub const SD_IO_CCCR_CISPTR: i32 = 0x09;
/// `SD_IO_CCCR_SPEED`.
pub const SD_IO_CCCR_SPEED: i32 = 0x13;
/// `CCCR_SPEED_SHS`.
pub const CCCR_SPEED_SHS: u8 = 1 << 0;
/// `CCCR_SPEED_EHS`.
pub const CCCR_SPEED_EHS: u8 = CCCR_SPEED_SDR25;
/// `CCCR_SPEED_SDR12`.
pub const CCCR_SPEED_SDR12: u8 = 0 << 1;
/// `CCCR_SPEED_SDR25`.
pub const CCCR_SPEED_SDR25: u8 = 1 << 1;
/// `CCCR_SPEED_SDR50`.
pub const CCCR_SPEED_SDR50: u8 = 2 << 1;
/// `CCCR_SPEED_SDR104`.
pub const CCCR_SPEED_SDR104: u8 = 3 << 1;
/// `CCCR_SPEED_DDR50`.
pub const CCCR_SPEED_DDR50: u8 = 4 << 1;
/// `CCCR_SPEED_MASK`.
pub const CCCR_SPEED_MASK: u8 = 0x7 << 1;

/* Function Basic Registers (FBR) */

/// `SD_IO_FBR_BLOCKLEN`.
pub const SD_IO_FBR_BLOCKLEN: i32 = 0x10;

/* Card Information Structure (CIS) */

/// `SD_IO_CIS_START`.
pub const SD_IO_CIS_START: i32 = 0x01000;
/// `SD_IO_CIS_SIZE`.
pub const SD_IO_CIS_SIZE: i32 = 0x17000;

/* CIS tuple codes (based on PC Card 16) */

/// `SD_IO_CISTPL_NULL`.
pub const SD_IO_CISTPL_NULL: u8 = 0x00;
/// `SD_IO_CISTPL_VERS_1`.
pub const SD_IO_CISTPL_VERS_1: u8 = 0x15;
/// `SD_IO_CISTPL_MANFID`.
pub const SD_IO_CISTPL_MANFID: u8 = 0x20;
/// `SD_IO_CISTPL_FUNCID`.
pub const SD_IO_CISTPL_FUNCID: u8 = 0x21;
/// `SD_IO_CISTPL_FUNCE`.
pub const SD_IO_CISTPL_FUNCE: u8 = 0x22;
/// `SD_IO_CISTPL_END`.
pub const SD_IO_CISTPL_END: u8 = 0xff;

/* CISTPL_FUNCID codes */

/// `TPLFID_FUNCTION_SDIO`.
pub const TPLFID_FUNCTION_SDIO: u8 = 0x0c;

/// `SD_R5_DATA(resp)`.
pub const fn sd_r5_data(resp: &[u32]) -> u8 {
    (resp[0] & 0xff) as u8
}

/* 48-bit response decoding (32 bits w/o CRC) */

/// `MMC_R4(resp)`.
pub const fn mmc_r4(resp: &[u32]) -> u32 {
    resp[0]
}

/// `MMC_R5(resp)`.
pub const fn mmc_r5(resp: &[u32]) -> u32 {
    resp[0]
}

/// `SD_IO_OCR_NUM_FUNCTIONS(ocr)`.
pub const fn sd_io_ocr_num_functions(ocr: u32) -> i32 {
    ((ocr >> 28) & 0x7) as i32
}

/// `SD_IO_FBR_BASE(f)`.
pub const fn sd_io_fbr_base(f: i32) -> i32 {
    f * 0x100
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest;

    #[test]
    fn macros_decode_their_fields() {
        assert_eq!(sd_r5_data(&[0x1234, 0, 0, 0]), 0x34);
        assert_eq!(sd_io_ocr_num_functions(0x3000_0000), 3);
        assert_eq!(sd_io_ocr_num_functions(0xf000_0000), 7);
        assert_eq!(sd_io_fbr_base(2) + SD_IO_FBR_BLOCKLEN, 0x210);
    }

    #[test]
    #[ignore = "reads the C reference (just test-ref)"]
    fn defines_match_the_reference() {
        let defs = reftest::defines("sys/dev/sdmmc/sdmmc_ioreg.h");
        #[allow(clippy::unnecessary_cast)] // a column of mixed types
        let ours: &[(&str, i64)] = &[
            ("SD_IO_SEND_OP_COND", SD_IO_SEND_OP_COND as i64),
            ("SD_IO_RW_DIRECT", SD_IO_RW_DIRECT as i64),
            ("SD_IO_RW_EXTENDED", SD_IO_RW_EXTENDED as i64),
            ("SD_ARG_CMD52_READ", SD_ARG_CMD52_READ as i64),
            ("SD_ARG_CMD52_WRITE", SD_ARG_CMD52_WRITE as i64),
            ("SD_ARG_CMD52_FUNC_SHIFT", SD_ARG_CMD52_FUNC_SHIFT as i64),
            ("SD_ARG_CMD52_FUNC_MASK", SD_ARG_CMD52_FUNC_MASK as i64),
            ("SD_ARG_CMD52_EXCHANGE", SD_ARG_CMD52_EXCHANGE as i64),
            ("SD_ARG_CMD52_REG_SHIFT", SD_ARG_CMD52_REG_SHIFT as i64),
            ("SD_ARG_CMD52_REG_MASK", SD_ARG_CMD52_REG_MASK as i64),
            ("SD_ARG_CMD52_DATA_SHIFT", SD_ARG_CMD52_DATA_SHIFT as i64),
            ("SD_ARG_CMD52_DATA_MASK", SD_ARG_CMD52_DATA_MASK as i64),
            ("SD_ARG_CMD53_READ", SD_ARG_CMD53_READ as i64),
            ("SD_ARG_CMD53_WRITE", SD_ARG_CMD53_WRITE as i64),
            ("SD_ARG_CMD53_FUNC_SHIFT", SD_ARG_CMD53_FUNC_SHIFT as i64),
            ("SD_ARG_CMD53_FUNC_MASK", SD_ARG_CMD53_FUNC_MASK as i64),
            ("SD_ARG_CMD53_BLOCK_MODE", SD_ARG_CMD53_BLOCK_MODE as i64),
            ("SD_ARG_CMD53_INCREMENT", SD_ARG_CMD53_INCREMENT as i64),
            ("SD_ARG_CMD53_REG_SHIFT", SD_ARG_CMD53_REG_SHIFT as i64),
            ("SD_ARG_CMD53_REG_MASK", SD_ARG_CMD53_REG_MASK as i64),
            (
                "SD_ARG_CMD53_LENGTH_SHIFT",
                SD_ARG_CMD53_LENGTH_SHIFT as i64,
            ),
            ("SD_ARG_CMD53_LENGTH_MASK", SD_ARG_CMD53_LENGTH_MASK as i64),
            ("SD_ARG_CMD53_LENGTH_MAX", SD_ARG_CMD53_LENGTH_MAX as i64),
            ("SD_IO_OCR_MEM_READY", SD_IO_OCR_MEM_READY as i64),
            ("SD_IO_OCR_MEM_PRESENT", SD_IO_OCR_MEM_PRESENT as i64),
            ("SD_IO_OCR_MASK", SD_IO_OCR_MASK as i64),
            ("SD_IO_CCCR_START", SD_IO_CCCR_START as i64),
            ("SD_IO_CCCR_SIZE", SD_IO_CCCR_SIZE as i64),
            ("SD_IO_CCCR_FN_ENABLE", SD_IO_CCCR_FN_ENABLE as i64),
            ("SD_IO_CCCR_FN_READY", SD_IO_CCCR_FN_READY as i64),
            ("SD_IO_CCCR_INT_ENABLE", SD_IO_CCCR_INT_ENABLE as i64),
            ("SD_IO_CCCR_CTL", SD_IO_CCCR_CTL as i64),
            ("CCCR_CTL_RES", CCCR_CTL_RES as i64),
            ("SD_IO_CCCR_BUS_WIDTH", SD_IO_CCCR_BUS_WIDTH as i64),
            ("CCCR_BUS_WIDTH_1", CCCR_BUS_WIDTH_1 as i64),
            ("CCCR_BUS_WIDTH_4", CCCR_BUS_WIDTH_4 as i64),
            ("CCCR_BUS_WIDTH_MASK", CCCR_BUS_WIDTH_MASK as i64),
            ("SD_IO_CCCR_CISPTR", SD_IO_CCCR_CISPTR as i64),
            ("SD_IO_CCCR_SPEED", SD_IO_CCCR_SPEED as i64),
            ("CCCR_SPEED_SHS", CCCR_SPEED_SHS as i64),
            ("CCCR_SPEED_EHS", CCCR_SPEED_EHS as i64),
            ("CCCR_SPEED_SDR12", CCCR_SPEED_SDR12 as i64),
            ("CCCR_SPEED_SDR25", CCCR_SPEED_SDR25 as i64),
            ("CCCR_SPEED_SDR50", CCCR_SPEED_SDR50 as i64),
            ("CCCR_SPEED_SDR104", CCCR_SPEED_SDR104 as i64),
            ("CCCR_SPEED_DDR50", CCCR_SPEED_DDR50 as i64),
            ("CCCR_SPEED_MASK", CCCR_SPEED_MASK as i64),
            ("SD_IO_FBR_BLOCKLEN", SD_IO_FBR_BLOCKLEN as i64),
            ("SD_IO_CIS_START", SD_IO_CIS_START as i64),
            ("SD_IO_CIS_SIZE", SD_IO_CIS_SIZE as i64),
            ("SD_IO_CISTPL_NULL", SD_IO_CISTPL_NULL as i64),
            ("SD_IO_CISTPL_VERS_1", SD_IO_CISTPL_VERS_1 as i64),
            ("SD_IO_CISTPL_MANFID", SD_IO_CISTPL_MANFID as i64),
            ("SD_IO_CISTPL_FUNCID", SD_IO_CISTPL_FUNCID as i64),
            ("SD_IO_CISTPL_FUNCE", SD_IO_CISTPL_FUNCE as i64),
            ("SD_IO_CISTPL_END", SD_IO_CISTPL_END as i64),
            ("TPLFID_FUNCTION_SDIO", TPLFID_FUNCTION_SDIO as i64),
        ];
        for &(name, v) in ours {
            assert_eq!(reftest::int(&defs, name), Some(v), "{name}");
        }
    }
}
/* </TESTS> */
