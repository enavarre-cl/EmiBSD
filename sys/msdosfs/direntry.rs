/*	$OpenBSD: direntry.h,v 1.8 2021/12/23 02:12:52 jsg Exp $	*/
/*	$NetBSD: direntry.h,v 1.13 1997/10/17 11:23:45 ws Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (C) 1994, 1995, 1997 Wolfgang Solfrank.
 * Copyright (C) 1994, 1995, 1997 TooLs GmbH.
 * All rights reserved.
 * Original code by Paul Popelka (paulp@uts.amdahl.com) (see below).
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *	This product includes software developed by TooLs GmbH.
 * 4. The name of TooLs GmbH may not be used to endorse or promote products
 *    derived from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY TOOLS GMBH ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
 * OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
 * IN NO EVENT SHALL TOOLS GMBH BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
 * OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
 * WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
 * OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF
 * ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */
/*
 * Written by Paul Popelka (paulp@uts.amdahl.com)
 *
 * You can do anything you want with this software, just don't say you wrote
 * it, and don't remove this notice.
 *
 * This software is provided "as is".
 *
 * The author supplies this software to be publicly redistributed on the
 * understanding that the author is not responsible for the correct
 * functioning of this software in any circumstances and is not liable for
 * any damages caused by this software.
 *
 * October 1992
 */
/* </LICENSES> */

/* <CODE> */
//! `<msdosfs/direntry.h>`: the structure of a DOS directory entry (`struct direntry`), the
//! Win95 long name entry (`struct winentry`) and the bit layout of the DOS time and date
//! fields.
//!
//! Upstream: sys/msdosfs/direntry.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - Both entries are viewed in place over a directory block's bytes with `at`/`at_mut`
//!   (`byte_view!` in `bpb.rs`), as the C casts a pointer into `b_data`; a `struct direntry *`
//!   that the C casts to `struct winentry *` is the same offset viewed as a [`Winentry`].
//! - `weReserved2` (`u_int16_t`) is a `[u8; 2]`: the structure keeps its 32-byte layout and
//!   the alignment of a byte, so a view may start at any offset. The kernel only ever stores
//!   0 there, which is the same in both forms.
//! - The C passes `dep->deName` where 11 bytes are meant (the name and the extension, which
//!   follows it): [`Direntry::name11`] and [`Direntry::set_name11`] are those 11 bytes.
//! - The `_KERNEL` prototypes (`unix2dostime`, `dos2unixfn`, `winChkName`, ...) are the
//!   functions of `msdosfs_conv.rs`, where the C file defines them.

use crate::msdosfs::bpb::byte_view;

/// `SLOT_EMPTY`: slot has never been used.
pub const SLOT_EMPTY: u8 = 0x00;
/// `SLOT_E5`: the real value is 0xe5.
pub const SLOT_E5: u8 = 0x05;
/// `SLOT_DELETED`: file in this slot deleted.
pub const SLOT_DELETED: u8 = 0xe5;

/// `ATTR_NORMAL`: normal file.
pub const ATTR_NORMAL: u8 = 0x00;
/// `ATTR_READONLY`: file is readonly.
pub const ATTR_READONLY: u8 = 0x01;
/// `ATTR_HIDDEN`: file is hidden.
pub const ATTR_HIDDEN: u8 = 0x02;
/// `ATTR_SYSTEM`: file is a system file.
pub const ATTR_SYSTEM: u8 = 0x04;
/// `ATTR_VOLUME`: entry is a volume label.
pub const ATTR_VOLUME: u8 = 0x08;
/// `ATTR_DIRECTORY`: entry is a directory name.
pub const ATTR_DIRECTORY: u8 = 0x10;
/// `ATTR_ARCHIVE`: file is new or modified.
pub const ATTR_ARCHIVE: u8 = 0x20;

/// `CASE_LOWER_BASE`: base is lower case.
pub const CASE_LOWER_BASE: u8 = 0x08;
/// `CASE_LOWER_EXT`: extension is lower case.
pub const CASE_LOWER_EXT: u8 = 0x10;

/// `struct direntry`: structure of a dos directory entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct Direntry {
    /// `deName`: filename, blank filled (`SLOT_*` in the first byte).
    pub deName: [u8; 8],
    /// `deExtension`: extension, blank filled.
    pub deExtension: [u8; 3],
    /// `deAttributes`: file attributes (`ATTR_*`).
    pub deAttributes: u8,
    /// `deLowerCase`: case for base and extension (`CASE_LOWER_*`).
    pub deLowerCase: u8,
    /// `deCTimeHundredth`: create time, 1/100th of a sec.
    pub deCTimeHundredth: u8,
    /// `deCTime`: create time.
    pub deCTime: [u8; 2],
    /// `deCDate`: create date.
    pub deCDate: [u8; 2],
    /// `deADate`: access date.
    pub deADate: [u8; 2],
    /// `deHighClust`: high byte of cluster number.
    pub deHighClust: [u8; 2],
    /// `deMTime`: last update time.
    pub deMTime: [u8; 2],
    /// `deMDate`: last update date.
    pub deMDate: [u8; 2],
    /// `deStartCluster`: starting cluster of file.
    pub deStartCluster: [u8; 2],
    /// `deFileSize`: size of file in bytes.
    pub deFileSize: [u8; 4],
}

impl Direntry {
    /// The 11 bytes the C reaches through `dep->deName`: the name and the extension.
    pub fn name11(&self) -> [u8; 11] {
        let mut n = [0u8; 11];
        n[..8].copy_from_slice(&self.deName);
        n[8..].copy_from_slice(&self.deExtension);
        n
    }

    /// Writes the 11 bytes of the name and the extension (`bcopy(name, dep->deName, 11)`).
    pub fn set_name11(&mut self, n: &[u8; 11]) {
        self.deName.copy_from_slice(&n[..8]);
        self.deExtension.copy_from_slice(&n[8..]);
    }
}

byte_view!(Direntry);

/// `WIN_LAST`: the last (first on disk) slot of a long name, in `weCnt`.
pub const WIN_LAST: u8 = 0x40;
/// `WIN_CNT`: the slot number, in `weCnt`.
pub const WIN_CNT: u8 = 0x3f;
/// `ATTR_WIN95`: the attributes that mark a long name slot.
pub const ATTR_WIN95: u8 = 0x0f;

/// `struct winentry`: structure of a Win95 long name directory entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_snake_case)] // the C field names
pub struct Winentry {
    /// `weCnt` (`WIN_LAST`, `WIN_CNT`).
    pub weCnt: u8,
    /// `wePart1`: name characters 1 to 5, UCS-2 little-endian.
    pub wePart1: [u8; 10],
    /// `weAttributes` (`ATTR_WIN95`).
    pub weAttributes: u8,
    /// `weReserved1`.
    pub weReserved1: u8,
    /// `weChksum`: the checksum of the short name.
    pub weChksum: u8,
    /// `wePart2`: name characters 6 to 11.
    pub wePart2: [u8; 12],
    /// `weReserved2` (a `u_int16_t`, see the module's deviations).
    pub weReserved2: [u8; 2],
    /// `wePart3`: name characters 12 and 13.
    pub wePart3: [u8; 4],
}

byte_view!(Winentry);

/// `WIN_CHARS`: number of chars per winentry.
pub const WIN_CHARS: usize = 13;

// This is the format of the contents of the deTime field in the direntry structure. We don't
// use bitfields because we don't know how compilers for arbitrary machines will lay them out.

/// `DT_2SECONDS_MASK`: seconds divided by 2.
pub const DT_2SECONDS_MASK: u32 = 0x1F;
/// `DT_2SECONDS_SHIFT`.
pub const DT_2SECONDS_SHIFT: u32 = 0;
/// `DT_MINUTES_MASK`: minutes.
pub const DT_MINUTES_MASK: u32 = 0x7E0;
/// `DT_MINUTES_SHIFT`.
pub const DT_MINUTES_SHIFT: u32 = 5;
/// `DT_HOURS_MASK`: hours.
pub const DT_HOURS_MASK: u32 = 0xF800;
/// `DT_HOURS_SHIFT`.
pub const DT_HOURS_SHIFT: u32 = 11;

// This is the format of the contents of the deDate field in the direntry structure.

/// `DD_DAY_MASK`: day of month.
pub const DD_DAY_MASK: u32 = 0x1F;
/// `DD_DAY_SHIFT`.
pub const DD_DAY_SHIFT: u32 = 0;
/// `DD_MONTH_MASK`: month.
pub const DD_MONTH_MASK: u32 = 0x1E0;
/// `DD_MONTH_SHIFT`.
pub const DD_MONTH_SHIFT: u32 = 5;
/// `DD_YEAR_MASK`: year - 1980.
pub const DD_YEAR_MASK: u32 = 0xFE00;
/// `DD_YEAR_SHIFT`.
pub const DD_YEAR_SHIFT: u32 = 9;

const _: () = {
    assert!(Direntry::SIZE == 32);
    assert!(Winentry::SIZE == 32);
    assert!(core::mem::offset_of!(Direntry, deHighClust) == 20);
    assert!(core::mem::offset_of!(Direntry, deStartCluster) == 26);
    assert!(core::mem::offset_of!(Winentry, wePart2) == 14);
    assert!(core::mem::offset_of!(Winentry, weReserved2) == 26);
    assert!(core::mem::offset_of!(Winentry, wePart3) == 28);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name11_spans_name_and_extension() {
        let mut blk = [b' '; 64];
        Direntry::at_mut(&mut blk, 32).set_name11(b"README  TXT");
        assert_eq!(&blk[32..43], b"README  TXT");
        assert_eq!(Direntry::at(&blk, 32).name11(), *b"README  TXT");
        assert_eq!(&Direntry::at(&blk, 32).deExtension, b"TXT");
        // The same bytes read as a long name slot.
        assert_eq!(Winentry::at(&blk, 32).weCnt, b'R');
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/msdosfs/direntry.h");
        for (name, value) in [
            ("SLOT_EMPTY", i64::from(SLOT_EMPTY)),
            ("SLOT_E5", i64::from(SLOT_E5)),
            ("SLOT_DELETED", i64::from(SLOT_DELETED)),
            ("ATTR_NORMAL", i64::from(ATTR_NORMAL)),
            ("ATTR_READONLY", i64::from(ATTR_READONLY)),
            ("ATTR_HIDDEN", i64::from(ATTR_HIDDEN)),
            ("ATTR_SYSTEM", i64::from(ATTR_SYSTEM)),
            ("ATTR_VOLUME", i64::from(ATTR_VOLUME)),
            ("ATTR_DIRECTORY", i64::from(ATTR_DIRECTORY)),
            ("ATTR_ARCHIVE", i64::from(ATTR_ARCHIVE)),
            ("CASE_LOWER_BASE", i64::from(CASE_LOWER_BASE)),
            ("CASE_LOWER_EXT", i64::from(CASE_LOWER_EXT)),
            ("WIN_LAST", i64::from(WIN_LAST)),
            ("WIN_CNT", i64::from(WIN_CNT)),
            ("ATTR_WIN95", i64::from(ATTR_WIN95)),
            ("WIN_CHARS", WIN_CHARS as i64),
            ("DT_2SECONDS_MASK", i64::from(DT_2SECONDS_MASK)),
            ("DT_MINUTES_MASK", i64::from(DT_MINUTES_MASK)),
            ("DT_MINUTES_SHIFT", i64::from(DT_MINUTES_SHIFT)),
            ("DT_HOURS_MASK", i64::from(DT_HOURS_MASK)),
            ("DT_HOURS_SHIFT", i64::from(DT_HOURS_SHIFT)),
            ("DD_DAY_MASK", i64::from(DD_DAY_MASK)),
            ("DD_MONTH_MASK", i64::from(DD_MONTH_MASK)),
            ("DD_MONTH_SHIFT", i64::from(DD_MONTH_SHIFT)),
            ("DD_YEAR_MASK", i64::from(DD_YEAR_MASK)),
            ("DD_YEAR_SHIFT", i64::from(DD_YEAR_SHIFT)),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
/* </TESTS> */
