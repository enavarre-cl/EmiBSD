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

/*-
 * Copyright (c) 1991 The Regents of the University of California.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. Neither the name of the University nor the names of its contributors
 *    may be used to endorse or promote products derived from this software
 *    without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE REGENTS AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE REGENTS OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	from: @(#)nec765.h	7.1 (Berkeley) 5/9/91
 *	$FreeBSD: nec765.h,v 1.4 1995/01/06 15:20:00 joerg Exp $
 */
/* </LICENSES> */

/* <CODE> */
//! The NEC 765 floppy disk controller: `<dev/ic/nec765reg.h>`.
//!
//! Upstream: sys/dev/ic/nec765reg.h @ 3ce1f3f79392
//!
//! The main status register's bits, the four result status registers' bits (with the `%b`
//! descriptions `fdcstatus` and `fdretry` print them with), the commands and the `specify`
//! encoding. fdc(4) and fd(4) drive the PC's 765-compatible controller with them
//! (`fdreg.h` includes this header).
//!
//! ## Deviations
//! - The `NE7_ST*BITS` `%b` strings are byte strings for `subr_prf`'s `Bitmask` (the C's
//!   octal escapes in hexadecimal).
//! - `NE7_SPEC_1(srt, hut)` and `NE7_SPEC_2(hlt, nd)` are the `const fn`s `ne7_spec_1` and
//!   `ne7_spec_2`.

// Main status register

/// `NE7_DAB`: Diskette drive A is seeking, thus busy.
pub const NE7_DAB: u8 = 0x01;
/// `NE7_DBB`: Diskette drive B is seeking, thus busy.
pub const NE7_DBB: u8 = 0x02;
/// `NE7_CB`: Diskette Controller Busy.
pub const NE7_CB: u8 = 0x10;
/// `NE7_NDM`: Diskette Controller in Non Dma Mode.
pub const NE7_NDM: u8 = 0x20;
/// `NE7_DIO`: Diskette Controller Data register I/O.
pub const NE7_DIO: u8 = 0x40;
/// `NE7_RQM`: Diskette Controller ReQuest for Master.
pub const NE7_RQM: u8 = 0x80;

// Status register ST0

/// `NE7_ST0BITS`.
pub const NE7_ST0BITS: &[u8] =
    b"\x10\x08invld\x07abnrml\x06seek_cmplt\x05equ_chck\x04drive_notrdy\x03top_head";

/// `NE7_ST0_IC`: interrupt completion code.
pub const NE7_ST0_IC: u8 = 0xc0;

/// `NE7_ST0_IC_RC`: terminated due to ready changed, n/a.
pub const NE7_ST0_IC_RC: u8 = 0xc0;
/// `NE7_ST0_IC_IV`: invalid command; must reset FDC.
pub const NE7_ST0_IC_IV: u8 = 0x80;
/// `NE7_ST0_IC_AT`: abnormal termination, check error stat.
pub const NE7_ST0_IC_AT: u8 = 0x40;
/// `NE7_ST0_IC_NT`: normal termination.
pub const NE7_ST0_IC_NT: u8 = 0x00;

/// `NE7_ST0_SE`: seek end.
pub const NE7_ST0_SE: u8 = 0x20;
/// `NE7_ST0_EC`: equipment check, recalibrated but no trk0.
pub const NE7_ST0_EC: u8 = 0x10;
/// `NE7_ST0_NR`: not ready (n/a).
pub const NE7_ST0_NR: u8 = 0x08;
/// `NE7_ST0_HD`: upper head selected.
pub const NE7_ST0_HD: u8 = 0x04;
/// `NE7_ST0_DR`: drive code.
pub const NE7_ST0_DR: u8 = 0x03;

// Status register ST1

/// `NE7_ST1BITS`.
pub const NE7_ST1BITS: &[u8] =
    b"\x10\x08end_of_cyl\x06bad_crc\x05data_overrun\x03sec_not_fnd\x02write_protect\x01no_am";

/// `NE7_ST1_EN`: end of cylinder, access past last record.
pub const NE7_ST1_EN: u8 = 0x80;
/// `NE7_ST1_DE`: data error, CRC fail in ID or data.
pub const NE7_ST1_DE: u8 = 0x20;
/// `NE7_ST1_OR`: DMA overrun, DMA failed to do i/o quickly.
pub const NE7_ST1_OR: u8 = 0x10;
/// `NE7_ST1_ND`: no data, sector not found or CRC in ID f.
pub const NE7_ST1_ND: u8 = 0x04;
/// `NE7_ST1_NW`: not writeable, attempt to violate WP.
pub const NE7_ST1_NW: u8 = 0x02;
/// `NE7_ST1_MA`: missing address mark (in ID or data field).
pub const NE7_ST1_MA: u8 = 0x01;

// Status register ST2

/// `NE7_ST2BITS`.
pub const NE7_ST2BITS: &[u8] =
    b"\x10\x07ctrl_mrk\x06bad_crc\x05wrong_cyl\x04scn_eq\x03scn_not_fnd\x02bad_cyl\x01no_dam";

/// `NE7_ST2_CM`: control mark; found deleted data.
pub const NE7_ST2_CM: u8 = 0x40;
/// `NE7_ST2_DD`: data error in data field, CRC fail.
pub const NE7_ST2_DD: u8 = 0x20;
/// `NE7_ST2_WC`: wrong cylinder, ID field mismatches cmd.
pub const NE7_ST2_WC: u8 = 0x10;
/// `NE7_ST2_SH`: scan equal hit.
pub const NE7_ST2_SH: u8 = 0x08;
/// `NE7_ST2_SN`: scan not satisfied.
pub const NE7_ST2_SN: u8 = 0x04;
/// `NE7_ST2_BC`: bad cylinder, cylinder marked 0xff.
pub const NE7_ST2_BC: u8 = 0x02;
/// `NE7_ST2_MD`: missing address mark in data field.
pub const NE7_ST2_MD: u8 = 0x01;

// Status register ST3

/// `NE7_ST3BITS`.
pub const NE7_ST3BITS: &[u8] =
    b"\x10\x08fault\x07write_protect\x06drdy\x05tk0\x04two_side\x03side_sel\x02";

/// `NE7_ST3_FT`: fault; PC: n/a.
pub const NE7_ST3_FT: u8 = 0x80;
/// `NE7_ST3_WP`: write protected.
pub const NE7_ST3_WP: u8 = 0x40;
/// `NE7_ST3_RD`: ready; PC: always true.
pub const NE7_ST3_RD: u8 = 0x20;
/// `NE7_ST3_T0`: track 0.
pub const NE7_ST3_T0: u8 = 0x10;
/// `NE7_ST3_TS`: two-sided; PC: n/a.
pub const NE7_ST3_TS: u8 = 0x08;
/// `NE7_ST3_HD`: upper head select.
pub const NE7_ST3_HD: u8 = 0x04;
/// `NE7_ST3_US`: unit select.
pub const NE7_ST3_US: u8 = 0x03;

// Commands
//
// the top three bits -- where appropriate -- are set as follows:
//
// 0x80 - MT  multi-track; allow both sides to be handled in single cmd
// 0x40 - MFM modified frequency modulation; use MFM encoding
// 0x20 - SK  skip; skip sectors marked as "deleted"

/// `NE7CMD_READTRK`: read whole track.
pub const NE7CMD_READTRK: u8 = 0x42;
/// `NE7CMD_SPECIFY`: specify drive parameters - requires unit parameters byte.
pub const NE7CMD_SPECIFY: u8 = 3;
/// `NE7CMD_SENSED`: sense drive - requires unit select byte.
pub const NE7CMD_SENSED: u8 = 4;
/// `NE7CMD_WRITE`: write - requires eight additional bytes.
pub const NE7CMD_WRITE: u8 = 0xc5;
/// `NE7CMD_READ`: read - requires eight additional bytes.
pub const NE7CMD_READ: u8 = 0xe6;
/// `NE7CMD_RECAL`: recalibrate drive - requires unit select byte.
pub const NE7CMD_RECAL: u8 = 7;
/// `NE7CMD_SENSEI`: sense controller interrupt status.
pub const NE7CMD_SENSEI: u8 = 8;
/// `NE7CMD_WRITEDEL`: write deleted data.
pub const NE7CMD_WRITEDEL: u8 = 0xc9;
/// `NE7CMD_READID`: read ID field.
pub const NE7CMD_READID: u8 = 0x4a;
/// `NE7CMD_READDEL`: read deleted data.
pub const NE7CMD_READDEL: u8 = 0xec;
/// `NE7CMD_FORMAT`: format - requires five additional bytes.
pub const NE7CMD_FORMAT: u8 = 0x4d;
/// `NE7CMD_SEEK`: seek drive - requires unit select byte and new cyl byte.
pub const NE7CMD_SEEK: u8 = 0x0f;
/// `NE7CMD_SCNEQU`: scan equal.
pub const NE7CMD_SCNEQU: u8 = 0xf1;
/// `NE7CMD_SCNLE`: scan less or equal.
pub const NE7CMD_SCNLE: u8 = 0xf9;
/// `NE7CMD_SCNGE`: scan greater or equal.
pub const NE7CMD_SCNGE: u8 = 0xfd;

// Enhanced controller commands:

/// `NE7CMD_VERSION`: version (ok for all controllers).
pub const NE7CMD_VERSION: u8 = 0x10;

/// `NE7_SPEC_1(srt, hut)`: the first `specify` byte (times relative to a FDC clock of 8 MHz:
/// srt - step rate, PC usually 3 ms; hut - head unload time, PC usually maximum of 240 ms).
pub const fn ne7_spec_1(srt: u8, hut: u8) -> u8 {
    ((16 - srt) << 4) | (hut / 16)
}

/// `NE7_SPEC_2(hlt, nd)`: the second `specify` byte (hlt - head load time, PC usually minimum
/// of 2 ms; nd - no DMA flag, PC usually not set (0)).
pub const fn ne7_spec_2(hlt: u8, nd: u8) -> u8 {
    (hlt & 0xFE) | (nd & 1)
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::kern::subr_prf::Bitmask;
    use std::format;

    #[test]
    fn status_bits_print_as_the_c() {
        // fd.c's "hard error" line on QEMU (OpenBSD 8.0): st0 21<seek_cmplt> st1 0 st2 0.
        assert_eq!(format!("{}", Bitmask(0x21, NE7_ST0BITS)), "21<seek_cmplt>");
        assert_eq!(format!("{}", Bitmask(0, NE7_ST1BITS)), "0");
        assert_eq!(
            format!("{}", Bitmask(0x85, NE7_ST1BITS)),
            "85<end_of_cyl,sec_not_fnd,no_am>"
        );
        assert_eq!(
            format!("{}", Bitmask(0x41, NE7_ST2BITS)),
            "41<ctrl_mrk,no_dam>"
        );
        assert_eq!(format!("{}", Bitmask(0x30, NE7_ST3BITS)), "30<drdy,tk0>");
    }

    #[test]
    fn specify_bytes() {
        // The 1.44MB type's step rate byte (0xcf) is NE7_SPEC_1(4, 240).
        assert_eq!(ne7_spec_1(4, 240), 0xcf);
        assert_eq!(ne7_spec_2(2, 0), 2);
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/dev/ic/nec765reg.h");
        for (name, v) in [
            ("NE7_DAB", NE7_DAB),
            ("NE7_DBB", NE7_DBB),
            ("NE7_CB", NE7_CB),
            ("NE7_NDM", NE7_NDM),
            ("NE7_DIO", NE7_DIO),
            ("NE7_RQM", NE7_RQM),
            ("NE7_ST0_IC", NE7_ST0_IC),
            ("NE7_ST0_IC_RC", NE7_ST0_IC_RC),
            ("NE7_ST0_IC_IV", NE7_ST0_IC_IV),
            ("NE7_ST0_IC_AT", NE7_ST0_IC_AT),
            ("NE7_ST0_IC_NT", NE7_ST0_IC_NT),
            ("NE7_ST0_SE", NE7_ST0_SE),
            ("NE7_ST0_EC", NE7_ST0_EC),
            ("NE7_ST0_NR", NE7_ST0_NR),
            ("NE7_ST0_HD", NE7_ST0_HD),
            ("NE7_ST0_DR", NE7_ST0_DR),
            ("NE7_ST1_EN", NE7_ST1_EN),
            ("NE7_ST1_DE", NE7_ST1_DE),
            ("NE7_ST1_OR", NE7_ST1_OR),
            ("NE7_ST1_ND", NE7_ST1_ND),
            ("NE7_ST1_NW", NE7_ST1_NW),
            ("NE7_ST1_MA", NE7_ST1_MA),
            ("NE7_ST2_CM", NE7_ST2_CM),
            ("NE7_ST2_DD", NE7_ST2_DD),
            ("NE7_ST2_WC", NE7_ST2_WC),
            ("NE7_ST2_SH", NE7_ST2_SH),
            ("NE7_ST2_SN", NE7_ST2_SN),
            ("NE7_ST2_BC", NE7_ST2_BC),
            ("NE7_ST2_MD", NE7_ST2_MD),
            ("NE7_ST3_FT", NE7_ST3_FT),
            ("NE7_ST3_WP", NE7_ST3_WP),
            ("NE7_ST3_RD", NE7_ST3_RD),
            ("NE7_ST3_T0", NE7_ST3_T0),
            ("NE7_ST3_TS", NE7_ST3_TS),
            ("NE7_ST3_HD", NE7_ST3_HD),
            ("NE7_ST3_US", NE7_ST3_US),
            ("NE7CMD_READTRK", NE7CMD_READTRK),
            ("NE7CMD_SPECIFY", NE7CMD_SPECIFY),
            ("NE7CMD_SENSED", NE7CMD_SENSED),
            ("NE7CMD_WRITE", NE7CMD_WRITE),
            ("NE7CMD_READ", NE7CMD_READ),
            ("NE7CMD_RECAL", NE7CMD_RECAL),
            ("NE7CMD_SENSEI", NE7CMD_SENSEI),
            ("NE7CMD_WRITEDEL", NE7CMD_WRITEDEL),
            ("NE7CMD_READID", NE7CMD_READID),
            ("NE7CMD_READDEL", NE7CMD_READDEL),
            ("NE7CMD_FORMAT", NE7CMD_FORMAT),
            ("NE7CMD_SEEK", NE7CMD_SEEK),
            ("NE7CMD_SCNEQU", NE7CMD_SCNEQU),
            ("NE7CMD_SCNLE", NE7CMD_SCNLE),
            ("NE7CMD_SCNGE", NE7CMD_SCNGE),
            ("NE7CMD_VERSION", NE7CMD_VERSION),
        ] {
            assert_eq!(
                crate::reftest::int(&defs, name),
                Some(i64::from(v)),
                "{name}"
            );
        }
    }
}
/* </TESTS> */
