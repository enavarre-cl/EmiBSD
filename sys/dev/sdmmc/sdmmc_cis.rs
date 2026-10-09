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
//! Routines to decode the Card Information Structure of SD I/O cards: the CIS pointer of a
//! function, the tuples the bus uses (`FUNCID`, `MANFID`, `VERS_1`), and the quirks of
//! cards whose CIS lacks them.
//!
//! Upstream: sys/dev/sdmmc/sdmmc_cis.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `sdmmc_read_cis` returns `Result<(), Errno>` (`EIO` for the C's 1). The `VERS_1`
//!   strings are recorded as offsets into `cis1_info_buf` ([`Cis1Info::Buf`]), the quirk
//!   strings as [`Cis1Info::Static`].
//! - The tuple walk is [`sdmmc_parse_cis`], over a register read function, so the parser is
//!   tested on the host; `sdmmc_read_cis` gives it CMD52 reads of function 0.
//! - `DPRINTF`s (`SDMMC_DEBUG`, not configured) are comments.

use crate::dev::sdmmc::sdmmc_io::sdmmc_io_read_1;
use crate::dev::sdmmc::sdmmc_ioreg::{
    SD_IO_CCCR_CISPTR, SD_IO_CCCR_SIZE, SD_IO_CIS_SIZE, SD_IO_CIS_START, SD_IO_CISTPL_END,
    SD_IO_CISTPL_FUNCID, SD_IO_CISTPL_MANFID, SD_IO_CISTPL_NULL, SD_IO_CISTPL_VERS_1,
    TPLFID_FUNCTION_SDIO,
};
use crate::dev::sdmmc::sdmmcdevs::{SDMMC_PRODUCT_SPECTEC_SDW820, SDMMC_VENDOR_SPECTEC};
use crate::dev::sdmmc::sdmmcvar::{Cis1Info, SdmmcCis, SdmmcFunction};
use crate::kern::kern_rwlock::rw_assert_wrlock;
use crate::kern::subr_prf::{Str, printf};
use crate::sys::errno::Errno;

/// `sdmmc_cisptr`: the function's CIS pointer, from its CCCR/FBR.
pub fn sdmmc_cisptr(sf: &SdmmcFunction) -> u32 {
    let sf0 = sf.sc.fn0();
    let mut cisptr: u32 = 0;

    rw_assert_wrlock(&sf.sc.sc_lock);

    let reg = SD_IO_CCCR_CISPTR + (sf.number.get() * SD_IO_CCCR_SIZE);
    cisptr |= u32::from(sdmmc_io_read_1(sf0, reg));
    cisptr |= u32::from(sdmmc_io_read_1(sf0, reg + 1)) << 8;
    cisptr |= u32::from(sdmmc_io_read_1(sf0, reg + 2)) << 16;

    cisptr
}

/// `sdmmc_read_cis`: decodes the function's CIS into `cis`.
pub fn sdmmc_read_cis(sf: &SdmmcFunction, cis: &mut SdmmcCis) -> Result<(), Errno> {
    let sf0 = sf.sc.fn0();

    rw_assert_wrlock(&sf.sc.sc_lock);

    let reg = sdmmc_cisptr(sf) as i32;
    sdmmc_parse_cis(sf.sc.devname(), reg, cis, |r| sdmmc_io_read_1(sf0, r))
}

/// The tuple walk of `sdmmc_read_cis` from CIS address `reg`, each byte read by `read`.
pub fn sdmmc_parse_cis(
    devname: &str,
    reg: i32,
    cis: &mut SdmmcCis,
    mut read: impl FnMut(i32) -> u8,
) -> Result<(), Errno> {
    let mut reg = reg;
    if !(SD_IO_CIS_START..(SD_IO_CIS_START + SD_IO_CIS_SIZE - 16)).contains(&reg) {
        printf(format_args!("{}: bad CIS ptr {:#x}\n", devname, reg));
        return Err(Errno::EIO);
    }

    loop {
        let tplcode = read(reg);
        reg += 1;
        if tplcode == SD_IO_CISTPL_END {
            break;
        }
        if tplcode == SD_IO_CISTPL_NULL {
            continue;
        }

        let tpllen = read(reg);
        reg += 1;

        match tplcode {
            SD_IO_CISTPL_FUNCID => {
                if tpllen < 2 {
                    printf(format_args!("{}: bad CISTPL_FUNCID length\n", devname));
                    reg += i32::from(tpllen);
                    continue;
                }
                cis.function = read(reg);
                reg += i32::from(tpllen);
            }
            SD_IO_CISTPL_MANFID => {
                if tpllen < 4 {
                    printf(format_args!("{}: bad CISTPL_MANFID length\n", devname));
                    reg += i32::from(tpllen);
                    continue;
                }
                cis.manufacturer = u16::from(read(reg));
                cis.manufacturer |= u16::from(read(reg + 1)) << 8;
                cis.product = u16::from(read(reg + 2));
                cis.product |= u16::from(read(reg + 3)) << 8;
                reg += 4;
            }
            SD_IO_CISTPL_VERS_1 => {
                if tpllen < 2 {
                    printf(format_args!("{}: CISTPL_VERS_1 too short\n", devname));
                    reg += i32::from(tpllen);
                    continue;
                }

                cis.cis1_major = read(reg);
                reg += 1;
                cis.cis1_minor = read(reg);
                reg += 1;

                let mut count = 0;
                let mut start = 0;
                let mut i = 0usize;
                while count < 4 && (i + 4) < 256 {
                    let ch = read(reg + i as i32);
                    if ch == 0xff {
                        break;
                    }
                    cis.cis1_info_buf[i] = ch;
                    if ch == 0 {
                        cis.cis1_info[count] = Some(Cis1Info::Buf(start));
                        start = i + 1;
                        count += 1;
                    }
                    i += 1;
                }

                reg += i32::from(tpllen) - 2;
            }
            _ => {
                // DPRINTF(("%s: unknown tuple code %#x, length %d\n", DEVNAME(sf->sc),
                //     tplcode, tpllen));
                reg += i32::from(tpllen);
            }
        }
    }

    Ok(())
}

/// `sdmmc_print_cis`.
pub fn sdmmc_print_cis(sf: &SdmmcFunction) {
    let cis = sf.cis.get();
    let name = sf.sc.devname();

    printf(format_args!(
        "{}: CIS version {}.{}\n",
        name, cis.cis1_major, cis.cis1_minor
    ));

    printf(format_args!("{}: CIS info: ", name));
    for i in 0..4 {
        let Some(info) = cis.info(i) else {
            break;
        };
        if i != 0 {
            printf(format_args!(", "));
        }
        printf(format_args!("{}", Str(info)));
    }
    printf(format_args!("\n"));

    printf(format_args!(
        "{}: Manufacturer code 0x{:x}, product 0x{:x}\n",
        name, cis.manufacturer, cis.product
    ));

    printf(format_args!("{}: function {}: ", name, sf.number.get()));
    match cis.function {
        TPLFID_FUNCTION_SDIO => {
            printf(format_args!("SDIO"));
        }
        f => {
            printf(format_args!("unknown ({})", f));
        }
    }
    printf(format_args!("\n"));
}

/// `sdmmc_check_cis_quirks`.
pub fn sdmmc_check_cis_quirks(sf: &SdmmcFunction) {
    let mut cis = sf.cis.get();
    if cis.manufacturer == SDMMC_VENDOR_SPECTEC && cis.product == SDMMC_PRODUCT_SPECTEC_SDW820 {
        // This card lacks the VERS_1 tuple.
        cis.cis1_major = 0x01;
        cis.cis1_minor = 0x00;
        cis.cis1_info[0] = Some(Cis1Info::Static(b"Spectec"));
        cis.cis1_info[1] = Some(Cis1Info::Static(b"SDIO WLAN Card"));
        cis.cis1_info[2] = Some(Cis1Info::Static(b"SDW-820"));
        cis.cis1_info[3] = Some(Cis1Info::Static(b""));
        sf.cis.set(cis);
    }
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cis_is_parsed() {
        let mut mem = [0u8; 0x1100];
        let tuples: &[u8] = &[
            0x00, // NULL
            0x21, 0x02, 0x0c, 0x00, // FUNCID: SDIO
            0x20, 0x04, 0xd0, 0x02, 0x29, 0x43, // MANFID: 0x02d0, 0x4329
            0x15, 0x0a, 0x01, 0x00, b'a', b'b', 0, b'c', 0, 0xff, 0, 0, // VERS_1
            0x99, 0x01, 0x00, // unknown tuple, skipped
            0xff, // END
        ];
        mem[0x1000..0x1000 + tuples.len()].copy_from_slice(tuples);
        let mut cis = SdmmcCis::new();
        let r = sdmmc_parse_cis("sdmmc0", 0x1000, &mut cis, |r| mem[r as usize]);
        assert!(r.is_ok());
        assert_eq!(cis.function, TPLFID_FUNCTION_SDIO);
        assert_eq!((cis.manufacturer, cis.product), (0x02d0, 0x4329));
        assert_eq!((cis.cis1_major, cis.cis1_minor), (1, 0));
        assert_eq!(cis.info(0), Some(&b"ab"[..]));
        assert_eq!(cis.info(1), Some(&b"c"[..]));
        assert_eq!(cis.info(2), None);
    }

    #[test]
    fn a_bad_cis_pointer_is_refused() {
        let mut cis = SdmmcCis::new();
        assert!(sdmmc_parse_cis("sdmmc0", 0x10, &mut cis, |_| 0xff).is_err());
        assert!(sdmmc_parse_cis("sdmmc0", 0x17ff0, &mut cis, |_| 0xff).is_err());
        assert!(sdmmc_parse_cis("sdmmc0", 0x1000, &mut cis, |_| 0xff).is_ok());
    }
}
/* </TESTS> */
