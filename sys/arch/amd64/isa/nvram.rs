/* <LICENSES> */
/*-
 * Copyright (c) 1990, 1993
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley by
 * William Jolitz.
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
 *	@(#)rtc.h	8.1 (Berkeley) 6/11/93
 */
/* </LICENSES> */

/* <CODE> */
//! The non-volatile RAM of the PC's RTC: `arch/amd64/isa/nvram.h`.
//!
//! Upstream: sys/arch/amd64/isa/nvram.h @ 3ce1f3f79392
//!
//! The following information is found in the non-volatile RAM in the MC146818A (or DS1287A or
//! other compatible) RTC on AT-compatible PCs.
//!
//! Status: `ported`.
//!
//! ## Deviations
//! - `NVRAM_DIAG_BITS`, the `%b` format string for the BIOS diagnostic byte, is a byte string.
//! - `MVRAM_EQUIPMENT_NFDS` keeps the C header's spelling (`MVRAM`, a typo for `NVRAM`).

use crate::dev::ic::mc146818reg::MC_NVRAM_START;

/// NVRAM byte 0: bios diagnostic (RTC offset 0xe).
pub const NVRAM_DIAG: u32 = MC_NVRAM_START;

/// `NVRAM_DIAG_BITS`: the `%b` description of the diagnostic byte.
pub const NVRAM_DIAG_BITS: &[u8] = b"\x10\x08clock_battery\x07ROM_cksum\x06config_unit\x05memory_size\x04fixed_disk\x03invalid_time";

/// NVRAM byte 1: reset code (RTC offset 0xf).
pub const NVRAM_RESET: u32 = MC_NVRAM_START + 1;

/// Normal reset.
pub const NVRAM_RESET_RST: u32 = 0x00;
/// Load system.
pub const NVRAM_RESET_LOAD: u32 = 0x04;
/// Jump through 40:67.
pub const NVRAM_RESET_JUMP: u32 = 0x0a;

/// NVRAM byte 2: diskette drive type in upper/lower nibble (RTC offset 0x10).
pub const NVRAM_DISKETTE: u32 = MC_NVRAM_START + 2;

/// None present.
pub const NVRAM_DISKETTE_NONE: u32 = 0;
/// 360K.
pub const NVRAM_DISKETTE_360K: u32 = 0x10;
/// 1.2M.
pub const NVRAM_DISKETTE_12M: u32 = 0x20;
/// 720K.
pub const NVRAM_DISKETTE_720K: u32 = 0x30;
/// 1.44M.
pub const NVRAM_DISKETTE_144M: u32 = 0x40;
/// 2.88M, presumably.
pub const NVRAM_DISKETTE_TYPE5: u32 = 0x50;
/// 2.88M.
pub const NVRAM_DISKETTE_TYPE6: u32 = 0x60;

/// NVRAM byte 6: equipment type.
pub const NVRAM_EQUIPMENT: u32 = MC_NVRAM_START + 6;

/// Floppy installed.
pub const NVRAM_EQUIPMENT_FLOPPY: u32 = 0x01;
/// FPU installed.
pub const NVRAM_EQUIPMENT_FPU: u32 = 0x02;
/// Keyboard installed.
pub const NVRAM_EQUIPMENT_KBD: u32 = 0x04;
/// Display installed.
pub const NVRAM_EQUIPMENT_DISPLAY: u32 = 0x08;
/// EGA or VGA.
pub const NVRAM_EQUIPMENT_EGAVGA: u32 = 0x00;
/// 40 column color.
pub const NVRAM_EQUIPMENT_COLOR40: u32 = 0x10;
/// 80 column color.
pub const NVRAM_EQUIPMENT_COLOR80: u32 = 0x20;
/// 80 column mono.
pub const NVRAM_EQUIPMENT_MONO80: u32 = 0x30;
/// Mask for monitor type.
pub const NVRAM_EQUIPMENT_MONITOR: u32 = 0x30;
/// Mask for # of floppies.
pub const MVRAM_EQUIPMENT_NFDS: u32 = 0xC0;

/// NVRAM byte 7: base memory size, low byte (RTC off. 0x15).
pub const NVRAM_BASELO: u32 = MC_NVRAM_START + 7;
/// NVRAM byte 8: base memory size, high byte (RTC off. 0x16).
pub const NVRAM_BASEHI: u32 = MC_NVRAM_START + 8;

/// NVRAM byte 9: extended memory size, low byte (RTC off. 0x17).
pub const NVRAM_EXTLO: u32 = MC_NVRAM_START + 9;
/// NVRAM byte 10: extended memory size, high byte (RTC off. 0x18).
pub const NVRAM_EXTHI: u32 = MC_NVRAM_START + 10;

/// NVRAM byte 34: extended memory POSTed size, low byte (RTC off. 0x30).
pub const NVRAM_PEXTLO: u32 = MC_NVRAM_START + 34;
/// NVRAM byte 35: extended memory POSTed size, high byte (RTC off. 0x31).
pub const NVRAM_PEXTHI: u32 = MC_NVRAM_START + 35;

/// NVRAM byte 36: current century. (please increment in Dec99!) (RTC offset 0x32).
pub const NVRAM_CENTURY: u32 = MC_NVRAM_START + 36;
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let mut defs = crate::reftest::defines("sys/arch/amd64/isa/nvram.h");
        defs.insert("MC_NVRAM_START".into(), MC_NVRAM_START.to_string());
        let ours: &[(&str, i64)] = &[
            ("NVRAM_DIAG", NVRAM_DIAG as i64),
            ("NVRAM_RESET", NVRAM_RESET as i64),
            ("NVRAM_RESET_RST", NVRAM_RESET_RST as i64),
            ("NVRAM_RESET_LOAD", NVRAM_RESET_LOAD as i64),
            ("NVRAM_RESET_JUMP", NVRAM_RESET_JUMP as i64),
            ("NVRAM_DISKETTE", NVRAM_DISKETTE as i64),
            ("NVRAM_DISKETTE_NONE", NVRAM_DISKETTE_NONE as i64),
            ("NVRAM_DISKETTE_360K", NVRAM_DISKETTE_360K as i64),
            ("NVRAM_DISKETTE_12M", NVRAM_DISKETTE_12M as i64),
            ("NVRAM_DISKETTE_720K", NVRAM_DISKETTE_720K as i64),
            ("NVRAM_DISKETTE_144M", NVRAM_DISKETTE_144M as i64),
            ("NVRAM_DISKETTE_TYPE5", NVRAM_DISKETTE_TYPE5 as i64),
            ("NVRAM_DISKETTE_TYPE6", NVRAM_DISKETTE_TYPE6 as i64),
            ("NVRAM_EQUIPMENT", NVRAM_EQUIPMENT as i64),
            ("NVRAM_EQUIPMENT_FLOPPY", NVRAM_EQUIPMENT_FLOPPY as i64),
            ("NVRAM_EQUIPMENT_FPU", NVRAM_EQUIPMENT_FPU as i64),
            ("NVRAM_EQUIPMENT_KBD", NVRAM_EQUIPMENT_KBD as i64),
            ("NVRAM_EQUIPMENT_DISPLAY", NVRAM_EQUIPMENT_DISPLAY as i64),
            ("NVRAM_EQUIPMENT_EGAVGA", NVRAM_EQUIPMENT_EGAVGA as i64),
            ("NVRAM_EQUIPMENT_COLOR40", NVRAM_EQUIPMENT_COLOR40 as i64),
            ("NVRAM_EQUIPMENT_COLOR80", NVRAM_EQUIPMENT_COLOR80 as i64),
            ("NVRAM_EQUIPMENT_MONO80", NVRAM_EQUIPMENT_MONO80 as i64),
            ("NVRAM_EQUIPMENT_MONITOR", NVRAM_EQUIPMENT_MONITOR as i64),
            ("MVRAM_EQUIPMENT_NFDS", MVRAM_EQUIPMENT_NFDS as i64),
            ("NVRAM_BASELO", NVRAM_BASELO as i64),
            ("NVRAM_BASEHI", NVRAM_BASEHI as i64),
            ("NVRAM_EXTLO", NVRAM_EXTLO as i64),
            ("NVRAM_EXTHI", NVRAM_EXTHI as i64),
            ("NVRAM_PEXTLO", NVRAM_PEXTLO as i64),
            ("NVRAM_PEXTHI", NVRAM_PEXTHI as i64),
            ("NVRAM_CENTURY", NVRAM_CENTURY as i64),
        ];
        for (name, value) in ours {
            assert_eq!(crate::reftest::int(&defs, name), Some(*value), "{name}");
        }
    }
}
/* </TESTS> */
