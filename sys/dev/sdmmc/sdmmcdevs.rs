/*
 * THIS FILE AUTOMATICALLY GENERATED.  DO NOT EDIT.
 *
 * generated from:
 *		OpenBSD: sdmmcdevs,v 1.9 2022/03/18 11:09:39 miod Exp
 */
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
//! `<dev/sdmmc/sdmmcdevs.h>`: the SDIO card vendor and product IDs of the CIS `MANFID`
//! tuple.
//!
//! Upstream: sys/dev/sdmmc/sdmmcdevs.h @ 3ce1f3f79392
//!
//! OpenBSD generates this header from `sys/dev/sdmmc/sdmmcdevs` with `devlist2h.awk`; here it
//! is written whole, with the generated names and values.
//!
//! ## Deviations
//! - The IDs are `u16`, the type of `struct sdmmc_cis`'s `manufacturer` and `product`.
/* List of known SD card vendors */
/// `SDMMC_VENDOR_CGUYS`: C-guys, Inc.
pub const SDMMC_VENDOR_CGUYS: u16 = 0x0092;
/// `SDMMC_VENDOR_TOSHIBA`: Toshiba.
pub const SDMMC_VENDOR_TOSHIBA: u16 = 0x0098;
/// `SDMMC_VENDOR_SOCKETCOM`: Socket Communications, Inc.
pub const SDMMC_VENDOR_SOCKETCOM: u16 = 0x0104;
/// `SDMMC_VENDOR_ATHEROS`: Atheros.
pub const SDMMC_VENDOR_ATHEROS: u16 = 0x0271;
/// `SDMMC_VENDOR_BROADCOM`: Broadcom.
pub const SDMMC_VENDOR_BROADCOM: u16 = 0x02d0;
/// `SDMMC_VENDOR_SYCHIP`: SyChip Inc.
pub const SDMMC_VENDOR_SYCHIP: u16 = 0x02db;
/// `SDMMC_VENDOR_SPECTEC`: Spectec Computer Co., Ltd.
pub const SDMMC_VENDOR_SPECTEC: u16 = 0x02fe;
/// `SDMMC_VENDOR_GLOBALSAT`: Globalsat Technology Co.
pub const SDMMC_VENDOR_GLOBALSAT: u16 = 0x0501;
/// `SDMMC_VENDOR_MEDIATEK`: MediaTek Inc.
pub const SDMMC_VENDOR_MEDIATEK: u16 = 0x037a;
/// `SDMMC_VENDOR_ABOCOM`: AboCom Systems, Inc.
pub const SDMMC_VENDOR_ABOCOM: u16 = 0x13d1;

/* List of known products, grouped by vendor */

/* AboCom Systems, Inc. */
/// `SDMMC_PRODUCT_ABOCOM_SDW11G`.
pub const SDMMC_PRODUCT_ABOCOM_SDW11G: u16 = 0xac02;

/* Atheros */
/// `SDMMC_PRODUCT_ATHEROS_AR6001_8`.
pub const SDMMC_PRODUCT_ATHEROS_AR6001_8: u16 = 0x0108;
/// `SDMMC_PRODUCT_ATHEROS_AR6001_9`.
pub const SDMMC_PRODUCT_ATHEROS_AR6001_9: u16 = 0x0109;
/// `SDMMC_PRODUCT_ATHEROS_AR6001_a`.
#[allow(non_upper_case_globals)] // the generated name
pub const SDMMC_PRODUCT_ATHEROS_AR6001_a: u16 = 0x010a;
/// `SDMMC_PRODUCT_ATHEROS_AR6001_b`.
#[allow(non_upper_case_globals)] // the generated name
pub const SDMMC_PRODUCT_ATHEROS_AR6001_b: u16 = 0x010b;

/* Broadcom */
/// `SDMMC_PRODUCT_BROADCOM_BCM4324`.
pub const SDMMC_PRODUCT_BROADCOM_BCM4324: u16 = 0x4324;
/// `SDMMC_PRODUCT_BROADCOM_BCM4329`.
pub const SDMMC_PRODUCT_BROADCOM_BCM4329: u16 = 0x4329;
/// `SDMMC_PRODUCT_BROADCOM_BCM4330`.
pub const SDMMC_PRODUCT_BROADCOM_BCM4330: u16 = 0x4330;
/// `SDMMC_PRODUCT_BROADCOM_BCM4334`.
pub const SDMMC_PRODUCT_BROADCOM_BCM4334: u16 = 0x4334;
/// `SDMMC_PRODUCT_BROADCOM_BCM4335`.
pub const SDMMC_PRODUCT_BROADCOM_BCM4335: u16 = 0x4335;
/// `SDMMC_PRODUCT_BROADCOM_BCM4339`.
pub const SDMMC_PRODUCT_BROADCOM_BCM4339: u16 = 0x4339;
/// `SDMMC_PRODUCT_BROADCOM_BCM4345`.
pub const SDMMC_PRODUCT_BROADCOM_BCM4345: u16 = 0x4345;
/// `SDMMC_PRODUCT_BROADCOM_BCM4354`.
pub const SDMMC_PRODUCT_BROADCOM_BCM4354: u16 = 0x4354;
/// `SDMMC_PRODUCT_BROADCOM_BCM4356`.
pub const SDMMC_PRODUCT_BROADCOM_BCM4356: u16 = 0x4356;
/// `SDMMC_PRODUCT_BROADCOM_BCM4359`.
pub const SDMMC_PRODUCT_BROADCOM_BCM4359: u16 = 0x4359;
/// `SDMMC_PRODUCT_BROADCOM_BCM43143`.
pub const SDMMC_PRODUCT_BROADCOM_BCM43143: u16 = 0xa887;
/// `SDMMC_PRODUCT_BROADCOM_BCM43340`.
pub const SDMMC_PRODUCT_BROADCOM_BCM43340: u16 = 0xa94c;
/// `SDMMC_PRODUCT_BROADCOM_BCM43341`.
pub const SDMMC_PRODUCT_BROADCOM_BCM43341: u16 = 0xa94d;
/// `SDMMC_PRODUCT_BROADCOM_BCM43362`.
pub const SDMMC_PRODUCT_BROADCOM_BCM43362: u16 = 0xa962;
/// `SDMMC_PRODUCT_BROADCOM_BCM43430`.
pub const SDMMC_PRODUCT_BROADCOM_BCM43430: u16 = 0xa9a6;
/// `SDMMC_PRODUCT_BROADCOM_BCM43364`.
pub const SDMMC_PRODUCT_BROADCOM_BCM43364: u16 = 0xa9bf;

/* C-guys, Inc. */
/// `SDMMC_PRODUCT_CGUYS_TIACX100`.
pub const SDMMC_PRODUCT_CGUYS_TIACX100: u16 = 0x0001;
/// `SDMMC_PRODUCT_CGUYS_SDFMRADIO2`.
pub const SDMMC_PRODUCT_CGUYS_SDFMRADIO2: u16 = 0x0005;
/// `SDMMC_PRODUCT_CGUYS_SDFMRADIO`.
pub const SDMMC_PRODUCT_CGUYS_SDFMRADIO: u16 = 0x5544;

/* Globalsat Technology Co. */
/// `SDMMC_PRODUCT_GLOBALSAT_SD501`.
pub const SDMMC_PRODUCT_GLOBALSAT_SD501: u16 = 0xf501;

/* MediaTek Inc. */
/// `SDMMC_PRODUCT_MEDIATEK_S2YWLAN`.
pub const SDMMC_PRODUCT_MEDIATEK_S2YWLAN: u16 = 0x5911;

/* Spectec Computer Co., Ltd */
/// `SDMMC_PRODUCT_SPECTEC_SDW820`.
pub const SDMMC_PRODUCT_SPECTEC_SDW820: u16 = 0x2128;

/* SyChip Inc. */
/// `SDMMC_PRODUCT_SYCHIP_WLAN6060SD`.
pub const SDMMC_PRODUCT_SYCHIP_WLAN6060SD: u16 = 0x0002;

/* Toshiba */
/// `SDMMC_PRODUCT_TOSHIBA_SDBTCARD1`.
pub const SDMMC_PRODUCT_TOSHIBA_SDBTCARD1: u16 = 0x0001;
/// `SDMMC_PRODUCT_TOSHIBA_SDBTCARD2`.
pub const SDMMC_PRODUCT_TOSHIBA_SDBTCARD2: u16 = 0x0002;
/// `SDMMC_PRODUCT_TOSHIBA_SDBTCARD3`.
pub const SDMMC_PRODUCT_TOSHIBA_SDBTCARD3: u16 = 0x0003;

/* Socket Communications, Inc. */
/// `SDMMC_PRODUCT_SOCKETCOM_SDSCANNER`.
pub const SDMMC_PRODUCT_SOCKETCOM_SDSCANNER: u16 = 0x005e;
/// `SDMMC_PRODUCT_SOCKETCOM_BTCARD`.
pub const SDMMC_PRODUCT_SOCKETCOM_BTCARD: u16 = 0x00c5;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::reftest;

    #[test]
    #[ignore = "reads the C reference (just test-ref)"]
    fn ids_match_the_generated_header() {
        let defs = reftest::defines("sys/dev/sdmmc/sdmmcdevs.h");
        let ours: &[(&str, u16)] = &[
            ("SDMMC_VENDOR_CGUYS", SDMMC_VENDOR_CGUYS),
            ("SDMMC_VENDOR_TOSHIBA", SDMMC_VENDOR_TOSHIBA),
            ("SDMMC_VENDOR_SOCKETCOM", SDMMC_VENDOR_SOCKETCOM),
            ("SDMMC_VENDOR_ATHEROS", SDMMC_VENDOR_ATHEROS),
            ("SDMMC_VENDOR_BROADCOM", SDMMC_VENDOR_BROADCOM),
            ("SDMMC_VENDOR_SYCHIP", SDMMC_VENDOR_SYCHIP),
            ("SDMMC_VENDOR_SPECTEC", SDMMC_VENDOR_SPECTEC),
            ("SDMMC_VENDOR_GLOBALSAT", SDMMC_VENDOR_GLOBALSAT),
            ("SDMMC_VENDOR_MEDIATEK", SDMMC_VENDOR_MEDIATEK),
            ("SDMMC_VENDOR_ABOCOM", SDMMC_VENDOR_ABOCOM),
            ("SDMMC_PRODUCT_ABOCOM_SDW11G", SDMMC_PRODUCT_ABOCOM_SDW11G),
            (
                "SDMMC_PRODUCT_ATHEROS_AR6001_8",
                SDMMC_PRODUCT_ATHEROS_AR6001_8,
            ),
            (
                "SDMMC_PRODUCT_ATHEROS_AR6001_9",
                SDMMC_PRODUCT_ATHEROS_AR6001_9,
            ),
            (
                "SDMMC_PRODUCT_ATHEROS_AR6001_a",
                SDMMC_PRODUCT_ATHEROS_AR6001_a,
            ),
            (
                "SDMMC_PRODUCT_ATHEROS_AR6001_b",
                SDMMC_PRODUCT_ATHEROS_AR6001_b,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM4324",
                SDMMC_PRODUCT_BROADCOM_BCM4324,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM4329",
                SDMMC_PRODUCT_BROADCOM_BCM4329,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM4330",
                SDMMC_PRODUCT_BROADCOM_BCM4330,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM4334",
                SDMMC_PRODUCT_BROADCOM_BCM4334,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM4335",
                SDMMC_PRODUCT_BROADCOM_BCM4335,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM4339",
                SDMMC_PRODUCT_BROADCOM_BCM4339,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM4345",
                SDMMC_PRODUCT_BROADCOM_BCM4345,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM4354",
                SDMMC_PRODUCT_BROADCOM_BCM4354,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM4356",
                SDMMC_PRODUCT_BROADCOM_BCM4356,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM4359",
                SDMMC_PRODUCT_BROADCOM_BCM4359,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM43143",
                SDMMC_PRODUCT_BROADCOM_BCM43143,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM43340",
                SDMMC_PRODUCT_BROADCOM_BCM43340,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM43341",
                SDMMC_PRODUCT_BROADCOM_BCM43341,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM43362",
                SDMMC_PRODUCT_BROADCOM_BCM43362,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM43430",
                SDMMC_PRODUCT_BROADCOM_BCM43430,
            ),
            (
                "SDMMC_PRODUCT_BROADCOM_BCM43364",
                SDMMC_PRODUCT_BROADCOM_BCM43364,
            ),
            ("SDMMC_PRODUCT_CGUYS_TIACX100", SDMMC_PRODUCT_CGUYS_TIACX100),
            (
                "SDMMC_PRODUCT_CGUYS_SDFMRADIO2",
                SDMMC_PRODUCT_CGUYS_SDFMRADIO2,
            ),
            (
                "SDMMC_PRODUCT_CGUYS_SDFMRADIO",
                SDMMC_PRODUCT_CGUYS_SDFMRADIO,
            ),
            (
                "SDMMC_PRODUCT_GLOBALSAT_SD501",
                SDMMC_PRODUCT_GLOBALSAT_SD501,
            ),
            (
                "SDMMC_PRODUCT_MEDIATEK_S2YWLAN",
                SDMMC_PRODUCT_MEDIATEK_S2YWLAN,
            ),
            ("SDMMC_PRODUCT_SPECTEC_SDW820", SDMMC_PRODUCT_SPECTEC_SDW820),
            (
                "SDMMC_PRODUCT_SYCHIP_WLAN6060SD",
                SDMMC_PRODUCT_SYCHIP_WLAN6060SD,
            ),
            (
                "SDMMC_PRODUCT_TOSHIBA_SDBTCARD1",
                SDMMC_PRODUCT_TOSHIBA_SDBTCARD1,
            ),
            (
                "SDMMC_PRODUCT_TOSHIBA_SDBTCARD2",
                SDMMC_PRODUCT_TOSHIBA_SDBTCARD2,
            ),
            (
                "SDMMC_PRODUCT_TOSHIBA_SDBTCARD3",
                SDMMC_PRODUCT_TOSHIBA_SDBTCARD3,
            ),
            (
                "SDMMC_PRODUCT_SOCKETCOM_SDSCANNER",
                SDMMC_PRODUCT_SOCKETCOM_SDSCANNER,
            ),
            (
                "SDMMC_PRODUCT_SOCKETCOM_BTCARD",
                SDMMC_PRODUCT_SOCKETCOM_BTCARD,
            ),
        ];
        for &(name, v) in ours {
            assert_eq!(reftest::int(&defs, name), Some(i64::from(v)), "{name}");
        }
        assert_eq!(defs.len(), ours.len());
    }
}
/* </TESTS> */
