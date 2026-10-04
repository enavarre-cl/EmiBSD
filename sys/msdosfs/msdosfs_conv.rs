/*	$OpenBSD: msdosfs_conv.c,v 1.22 2024/09/12 09:07:28 claudio Exp $	*/
/*	$NetBSD: msdosfs_conv.c,v 1.24 1997/10/17 11:23:54 ws Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (C) 1995, 1997 Wolfgang Solfrank.
 * Copyright (C) 1995, 1997 TooLs GmbH.
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

//! Conversions between unix and DOS: file times (`unix2dostime`, `dos2unixtime`), short file
//! names (`dos2unixfn`, `unix2dosfn`) and the Win95 long names (`unix2winfn`, `winChkName`,
//! `win2unixfn`, `winChksum`, `winSlotCnt`).
//!
//! Upstream: sys/msdosfs/msdosfs_conv.c @ 3ce1f3f79392
//!
//! DOS filenames are made of 2 parts, the name part and the extension part. The name part is 8
//! characters long and the extension part is 3 characters long. They may contain trailing
//! blanks if the name or extension are not long enough to fill their respective fields.
//!
//! ## Deviations
//! - The variables that remember the last time conversions (`lasttime`, `lastday`,
//!   `lastddate`, `lastdtime`; `lastdosdate`, `lastseconds`) are two `AtomicU64`s, each
//!   holding one conversion whole, so that a reader never pairs the key of one conversion with
//!   the result of another. `lastday` is not stored: it is always `lasttime / 86400` once a
//!   conversion was made, and `lasttime` (even, after `t &= ~1`) is kept halved. 0 means "no
//!   conversion yet", which the C's zeroed globals also mean (no valid input maps to it).
//! - `unix2dostime` returns the date, the time and the hundredths, where the C fills the
//!   `ddp`, `dtp` and `dhp` pointers (the last two only when given); `dos2unixtime` returns
//!   the `timespec`.
//! - `dos2unixtime` with a month above 12 (a corrupt entry) reads past the end of `regyear` or
//!   `leapyear` in the C; here those months count 0 days instead of panicking.
//! - The name functions take the user's name as a byte slice (the C's `un` and `unlen`), the
//!   DOS name as `[u8; 11]` and the long name entry as a `Winentry` view. `dos2unixfn`'s
//!   `lower` is a `bool` and its result a `usize`; `win2unixfn` fills a `Dirent`.
//! - `winChkName` with a slot numbered 0 (a corrupt entry; the C would compare bytes before
//!   the name) matches nothing.
//! - The three passes the C makes over `wePart1`, `wePart2` and `wePart3` are one pass over
//!   the 13 characters of the entry (`wchar`, `set_wchar`); `win2unixfn`'s length check,
//!   which the C compiles into the pass that holds character `WIN_MAXLEN % WIN_CHARS`, is
//!   applied in the same part.
//! - The `unix2dos`, `dos2unix` and `u2l` tables are written out once; a reference-backed test
//!   compares them with the C file's (`just test-ref`).

use core::sync::atomic::{AtomicU64, Ordering};

use crate::kern::subr_prf::printf;
use crate::msdosfs::denode::WIN_MAXLEN;
use crate::msdosfs::direntry::{
    ATTR_WIN95, DD_DAY_MASK, DD_DAY_SHIFT, DD_MONTH_MASK, DD_MONTH_SHIFT, DD_YEAR_MASK,
    DD_YEAR_SHIFT, DT_2SECONDS_MASK, DT_2SECONDS_SHIFT, DT_HOURS_MASK, DT_HOURS_SHIFT,
    DT_MINUTES_MASK, DT_MINUTES_SHIFT, SLOT_E5, WIN_CHARS, WIN_CNT, WIN_LAST, Winentry,
};
use crate::sys::dirent::Dirent;
use crate::sys::time::Timespec;

/// `regyear`: days in each month in a regular year.
pub const REGYEAR: [u16; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// `leapyear`: days in each month in a leap year.
pub const LEAPYEAR: [u16; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// `DOSEPOCH`: before 1/1/1980 there is only a timeless void.
const DOSEPOCH: i64 = 315532800;
/// `DOSENDTIME`: after 12/31/2107 there is only Cthulhu.
const DOSENDTIME: i64 = 4354775999;

/// `SECONDSTO1980`: the number of seconds between Jan 1, 1970 and Jan 1, 1980. In that
/// interval there were 8 regular years and 2 leap years.
const SECONDSTO1980: u32 = ((8 * 365) + (2 * 366)) * (24 * 60 * 60);

/// `unix2dos[]`: unix (ISO 8859-1) characters to DOS (code page 850) upper case; 0 for the
/// characters a short name cannot hold.
static UNIX2DOS: [u8; 256] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 00-07
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 08-0f
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 10-17
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 18-1f
    0x00, 0x21, 0x00, 0x23, 0x24, 0x25, 0x26, 0x27, // 20-27
    0x28, 0x29, 0x00, 0x00, 0x00, 0x2d, 0x00, 0x00, // 28-2f
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, // 30-37
    0x38, 0x39, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 38-3f
    0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, // 40-47
    0x48, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f, // 48-4f
    0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, // 50-57
    0x58, 0x59, 0x5a, 0x00, 0x00, 0x00, 0x5e, 0x5f, // 58-5f
    0x60, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, // 60-67
    0x48, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f, // 68-6f
    0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, // 70-77
    0x58, 0x59, 0x5a, 0x7b, 0x00, 0x7d, 0x7e, 0x00, // 78-7f
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 80-87
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 88-8f
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 90-97
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 98-9f
    0x00, 0xad, 0xbd, 0x9c, 0xcf, 0xbe, 0xdd, 0xf5, // a0-a7
    0xf9, 0xb8, 0xa6, 0xae, 0xaa, 0xf0, 0xa9, 0xee, // a8-af
    0xf8, 0xf1, 0xfd, 0xfc, 0xef, 0xe6, 0xf4, 0xfa, // b0-b7
    0xf7, 0xfb, 0xa7, 0xaf, 0xac, 0xab, 0xf3, 0xa8, // b8-bf
    0xb7, 0xb5, 0xb6, 0xc7, 0x8e, 0x8f, 0x92, 0x80, // c0-c7
    0xd4, 0x90, 0xd2, 0xd3, 0xde, 0xd6, 0xd7, 0xd8, // c8-cf
    0xd1, 0xa5, 0xe3, 0xe0, 0xe2, 0xe5, 0x99, 0x9e, // d0-d7
    0x9d, 0xeb, 0xe9, 0xea, 0x9a, 0xed, 0xe8, 0xe1, // d8-df
    0xb7, 0xb5, 0xb6, 0xc7, 0x8e, 0x8f, 0x92, 0x80, // e0-e7
    0xd4, 0x90, 0xd2, 0xd3, 0xde, 0xd6, 0xd7, 0xd8, // e8-ef
    0xd1, 0xa5, 0xe3, 0xe0, 0xe2, 0xe5, 0x99, 0xf6, // f0-f7
    0x9d, 0xeb, 0xe9, 0xea, 0x9a, 0xed, 0xe8, 0x98, // f8-ff
];

/// `dos2unix[]`: DOS (code page 850) characters to unix (ISO 8859-1); `?` for those that have
/// no counterpart.
static DOS2UNIX: [u8; 256] = [
    0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, // 00-07
    0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, // 08-0f
    0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, // 10-17
    0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, // 18-1f
    0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, // 20-27
    0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x3f, // 28-2f
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, // 30-37
    0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, // 38-3f
    0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, // 40-47
    0x48, 0x49, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f, // 48-4f
    0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, // 50-57
    0x58, 0x59, 0x5a, 0x5b, 0x5c, 0x5d, 0x5e, 0x5f, // 58-5f
    0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, // 60-67
    0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f, // 68-6f
    0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, // 70-77
    0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e, 0x7f, // 78-7f
    0xc7, 0xfc, 0xe9, 0xe2, 0xe4, 0xe0, 0xe5, 0xe7, // 80-87
    0xea, 0xeb, 0xe8, 0xef, 0xee, 0xec, 0xc4, 0xc5, // 88-8f
    0xc9, 0xe6, 0xc6, 0xf4, 0xf6, 0xf2, 0xfb, 0xf9, // 90-97
    0xff, 0xd6, 0xdc, 0xf8, 0xa3, 0xd8, 0xd7, 0x3f, // 98-9f
    0xe1, 0xed, 0xf3, 0xfa, 0xf1, 0xd1, 0xaa, 0xba, // a0-a7
    0xbf, 0xae, 0xac, 0xbd, 0xbc, 0xa1, 0xab, 0xbb, // a8-af
    0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0xc1, 0xc2, 0xc0, // b0-b7
    0xa9, 0x3f, 0x3f, 0x3f, 0x3f, 0xa2, 0xa5, 0x3f, // b8-bf
    0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0xe3, 0xc3, // c0-c7
    0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0x3f, 0xa4, // c8-cf
    0xf0, 0xd0, 0xca, 0xcb, 0xc8, 0x3f, 0xcd, 0xce, // d0-d7
    0xcf, 0x3f, 0x3f, 0x3f, 0x3f, 0xa6, 0xcc, 0x3f, // d8-df
    0xd3, 0xdf, 0xd4, 0xd2, 0xf5, 0xd5, 0xb5, 0xfe, // e0-e7
    0xde, 0xda, 0xdb, 0xd9, 0xfd, 0xdd, 0xaf, 0x3f, // e8-ef
    0xad, 0xb1, 0x3f, 0xbe, 0xb6, 0xa7, 0xf7, 0xb8, // f0-f7
    0xb0, 0xa8, 0xb7, 0xb9, 0xb3, 0xb2, 0x3f, 0x3f, // f8-ff
];

/// `u2l[]`: upper case to lower case (ISO 8859-1); `/` becomes `?`.
static U2L: [u8; 256] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, // 00-07
    0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, // 08-0f
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, // 10-17
    0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, // 18-1f
    0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, // 20-27
    0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x3f, // 28-2f
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, // 30-37
    0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f, // 38-3f
    0x40, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, // 40-47
    0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f, // 48-4f
    0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, // 50-57
    0x78, 0x79, 0x7a, 0x5b, 0x5c, 0x5d, 0x5e, 0x5f, // 58-5f
    0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, // 60-67
    0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f, // 68-6f
    0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, // 70-77
    0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e, 0x7f, // 78-7f
    0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, // 80-87
    0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f, // 88-8f
    0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, // 90-97
    0x98, 0x99, 0x9a, 0x9b, 0x9c, 0x9d, 0x9e, 0x9f, // 98-9f
    0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, // a0-a7
    0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf, // a8-af
    0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, // b0-b7
    0xb8, 0xb9, 0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf, // b8-bf
    0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, // c0-c7
    0xe8, 0xe9, 0xea, 0xeb, 0xec, 0xed, 0xee, 0xef, // c8-cf
    0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xd7, // d0-d7
    0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, 0xfe, 0xdf, // d8-df
    0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, // e0-e7
    0xe8, 0xe9, 0xea, 0xeb, 0xec, 0xed, 0xee, 0xef, // e8-ef
    0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, // f0-f7
    0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, 0xfe, 0xff, // f8-ff
];

/// `lasttime / 2`, `lastddate` and `lastdtime` of the last `unix2dostime` conversion, as
/// `t / 2 << 32 | ddate << 16 | dtime` (see the module's deviations).
static UNIX2DOS_LAST: AtomicU64 = AtomicU64::new(0);

/// `lastdosdate` and `lastseconds` of the last `dos2unixtime` conversion, as
/// `dd << 32 | seconds`.
static DOS2UNIX_LAST: AtomicU64 = AtomicU64::new(0);

/// `unix2dostime(tsp, ddp, dtp, dhp)`: convert the unix version of time to dos's idea of time
/// to be used in file timestamps. The passed in unix time is assumed to be in GMT. Returns the
/// DOS date, the DOS time and the hundredths of a second (`*ddp`, `*dtp`, `*dhp`).
pub fn unix2dostime(tsp: &Timespec) -> (u16, u16, u8) {
    // If the time from the last conversion is the same as now, then skip the computations and
    // use the saved result.
    let mut t = tsp.tv_sec & !1;
    if !(DOSEPOCH..=DOSENDTIME).contains(&t) {
        t = DOSEPOCH;
    }

    let last = UNIX2DOS_LAST.load(Ordering::Relaxed);
    let lasttime = (last >> 32) as i64 * 2;
    let (ddate, dtime) = if last != 0 && lasttime == t {
        ((last >> 16) as u16, last as u16)
    } else {
        let dtime = (((t / 2) % 30) << DT_2SECONDS_SHIFT)
            + (((t / 60) % 60) << DT_MINUTES_SHIFT)
            + (((t / 3600) % 24) << DT_HOURS_SHIFT);

        // If the number of days since 1970 is the same as the last time we did the
        // computation then skip all this leap year and month stuff.
        let mut days = (t / (24 * 60 * 60)) as u32;
        let ddate = if last != 0 && (lasttime / (24 * 60 * 60)) as u32 == days {
            (last >> 16) as u16
        } else {
            let mut year: u32 = 1970;
            loop {
                let inc = if year & 0x03 != 0 { 365 } else { 366 };
                if days < inc {
                    break;
                }
                days -= inc;
                year += 1;
            }
            let months = if year & 0x03 != 0 {
                &REGYEAR
            } else {
                &LEAPYEAR
            };
            let mut month: u32 = 0;
            while month < 12 {
                if days < u32::from(months[month as usize]) {
                    break;
                }
                days -= u32::from(months[month as usize]);
                month += 1;
            }
            let mut ddate = ((days + 1) << DD_DAY_SHIFT) + ((month + 1) << DD_MONTH_SHIFT);
            // Remember dos's idea of time is relative to 1980. unix's is relative to 1970.
            // If somehow we get a time before 1980 then don't give totally crazy results.
            if year > 1980 {
                ddate += (year - 1980) << DD_YEAR_SHIFT;
            }
            ddate as u16
        };
        let dtime = dtime as u16;
        UNIX2DOS_LAST.store(
            ((t / 2) as u64) << 32 | u64::from(ddate) << 16 | u64::from(dtime),
            Ordering::Relaxed,
        );
        (ddate, dtime)
    };

    let dh = ((tsp.tv_sec & 1) * 100 + tsp.tv_nsec / 10_000_000) as u8;
    (ddate, dtime, dh)
}

/// `dos2unixtime(dd, dt, dh, tsp)`: convert from dos' idea of time to unix'. This will
/// probably only be called from the stat(), and fstat() system calls and so probably need not
/// be too efficient.
pub fn dos2unixtime(dd: u32, dt: u32, dh: u32) -> Timespec {
    if dd == 0 {
        // Uninitialized field, return the epoch.
        return Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
    }
    let seconds = ((dt & DT_2SECONDS_MASK) >> DT_2SECONDS_SHIFT) * 2
        + ((dt & DT_MINUTES_MASK) >> DT_MINUTES_SHIFT) * 60
        + ((dt & DT_HOURS_MASK) >> DT_HOURS_SHIFT) * 3600
        + dh / 100;

    // If the year, month, and day from the last conversion are the same then use the saved
    // value.
    let last = DOS2UNIX_LAST.load(Ordering::Relaxed);
    let lastseconds = if last != 0 && (last >> 32) as u32 == dd {
        last as u32
    } else {
        let mut days: u32 = 0;
        let year = (dd & DD_YEAR_MASK) >> DD_YEAR_SHIFT;
        for y in 0..year {
            days += if y & 0x03 != 0 { 365 } else { 366 };
        }
        let months = if year & 0x03 != 0 {
            &REGYEAR
        } else {
            &LEAPYEAR
        };
        // Prevent going from 0 to 0xffffffff in the following loop.
        let mut month = (dd & DD_MONTH_MASK) >> DD_MONTH_SHIFT;
        if month == 0 {
            printf(format_args!(
                "dos2unixtime(): month value out of range ({})\n",
                month
            ));
            month = 1;
        }
        for m in 0..month - 1 {
            days += months.get(m as usize).copied().map_or(0, u32::from);
        }
        days = days.wrapping_add(((dd & DD_DAY_MASK) >> DD_DAY_SHIFT).wrapping_sub(1));
        let lastseconds = days.wrapping_mul(24 * 60 * 60).wrapping_add(SECONDSTO1980);
        DOS2UNIX_LAST.store(
            u64::from(dd) << 32 | u64::from(lastseconds),
            Ordering::Relaxed,
        );
        lastseconds
    };
    Timespec {
        tv_sec: i64::from(seconds.wrapping_add(lastseconds)),
        tv_nsec: i64::from(dh % 100) * 10_000_000,
    }
}

/// `dos2unixfn(dn, un, lower)`: convert a DOS filename to a unix filename (NUL-terminated,
/// in lower case when `lower`), and return the number of characters in the resulting unix
/// filename excluding the terminating null. `un` must hold 13 bytes.
pub fn dos2unixfn(dn: &[u8; 11], un: &mut [u8], lower: bool) -> usize {
    let conv = |c: u8| if lower { U2L[usize::from(c)] } else { c };

    // If first char of the filename is SLOT_E5 (0x05), then the real first char of the
    // filename should be 0xe5. But, they couldn't just have a 0xe5 mean 0xe5 because that is
    // used to mean a freed directory slot. Another dos quirk.
    let c = if dn[0] == SLOT_E5 {
        DOS2UNIX[0xe5]
    } else {
        DOS2UNIX[usize::from(dn[0])]
    };
    un[0] = conv(c);
    let mut k = 1;
    let mut thislong = 1;

    // Copy the name portion into the unix filename string.
    for &d in dn[1..8].iter().take_while(|&&d| d != b' ') {
        un[k] = conv(DOS2UNIX[usize::from(d)]);
        k += 1;
        thislong += 1;
    }

    // Now, if there is an extension then put in a period and copy in the extension.
    if dn[8] != b' ' {
        un[k] = b'.';
        k += 1;
        thislong += 1;
        for &d in dn[8..11].iter().take_while(|&&d| d != b' ') {
            un[k] = conv(DOS2UNIX[usize::from(d)]);
            k += 1;
            thislong += 1;
        }
    }
    un[k] = 0;

    thislong
}

/// `unix2dosfn(un, dn, unlen, gen)`: convert a unix filename to a DOS filename according to
/// Win95 rules. If applicable and `gen` is not 0, it is inserted into the converted filename
/// as a generation number. Returns
/// - 0 if name couldn't be converted
/// - 1 if the converted name is the same as the original (no long filename entry necessary
///   for Win95)
/// - 2 if conversion was successful
/// - 3 if conversion was successful and generation number was inserted
pub fn unix2dosfn(un: &[u8], dn: &mut [u8; 11], mut r#gen: u32) -> i32 {
    let unlen = un.len();
    let mut conv = 1;

    // Fill the dos filename string with blanks. These are DOS's pad characters.
    dn.fill(b' ');

    // The filenames "." and ".." are handled specially, since they don't follow dos filename
    // rules.
    if un.first() == Some(&b'.') && unlen == 1 {
        dn[0] = b'.';
        return i32::from(r#gen <= 1);
    }
    if un.first() == Some(&b'.') && un.get(1) == Some(&b'.') && unlen == 2 {
        dn[0] = b'.';
        dn[1] = b'.';
        return i32::from(r#gen <= 1);
    }

    // Filenames with only blanks and dots are not allowed!
    if un.iter().all(|&c| c == b' ' || c == b'.') {
        return 0;
    }

    // Now find the extension. Note: dot as first char doesn't start extension and trailing
    // dots and blanks are ignored. `dp` is the index just past the dot that starts the
    // extension; `dp1` the index just past the first of the trailing dots.
    let mut dp: Option<usize> = None;
    let mut dp1: Option<usize> = None;
    for (idx, &c) in un.iter().enumerate().skip(1) {
        match c {
            b'.' => {
                if dp1.is_none() {
                    dp1 = Some(idx + 1);
                }
            }
            b' ' => {}
            _ => {
                if dp1.is_some() {
                    dp = dp1;
                }
                dp1 = None;
            }
        }
    }

    // Now convert it. A character the table maps to 0 is dropped (its slot is reused).
    let name_end = if let Some(d) = dp {
        let l = match dp1 {
            Some(d1) => d1 - d,
            None => unlen - d,
        };
        let mut i = 0;
        let mut j = 8;
        while i < l && j < 11 {
            let c = un[d + i];
            dn[j] = UNIX2DOS[usize::from(c)];
            if c != dn[j] && conv != 3 {
                conv = 2;
            }
            if dn[j] == 0 {
                conv = 3;
                dn[j] = b' ';
            } else {
                j += 1;
            }
            i += 1;
        }
        if i < l {
            conv = 3;
        }
        // The name ends at the dot.
        d - 1
    } else {
        let mut e = unlen;
        while e > 0 && (un[e - 1] == b' ' || un[e - 1] == b'.') {
            e -= 1;
        }
        e
    };

    // Now convert the rest of the name.
    let mut u = 0;
    let mut j = 0;
    while u < name_end && j < 8 {
        let c = un[u];
        dn[j] = UNIX2DOS[usize::from(c)];
        if c != dn[j] && conv != 3 {
            conv = 2;
        }
        if dn[j] == 0 {
            conv = 3;
            dn[j] = b' ';
        } else {
            j += 1;
        }
        u += 1;
    }
    if u < name_end {
        conv = 3;
    }
    // If we didn't have any chars in filename, generate a default.
    if j == 0 {
        dn[0] = b'_';
    }

    // The first character cannot be E5, because that means a deleted entry.
    if dn[0] == 0xe5 {
        dn[0] = SLOT_E5;
    }

    // If there wasn't any char dropped, there is no place for generation numbers.
    if conv != 3 {
        if r#gen > 1 {
            return 0;
        }
        return conv;
    }

    // Now insert the generation number into the filename part.
    let mut gentext = [0u8; 6];
    let mut cp = gentext.len();
    while cp > 0 && r#gen != 0 {
        cp -= 1;
        gentext[cp] = b'0' + (r#gen % 10) as u8;
        r#gen /= 10;
    }
    if r#gen != 0 {
        return 0;
    }
    let mut i = 8;
    while i > 0 {
        i -= 1;
        if dn[i] != b' ' {
            break;
        }
    }
    let glen = gentext.len() - cp;
    if glen + 1 > 8 - i {
        i = 8 - (glen + 1);
    }
    dn[i] = b'~';
    i += 1;
    for &g in &gentext[cp..] {
        dn[i] = g;
        i += 1;
    }
    3
}

/// Byte offset in a `struct winentry` of the UCS-2 character `pos` (0 to 12) of the name:
/// `wePart1` holds characters 0 to 4, `wePart2` 5 to 10, `wePart3` 11 and 12.
fn wchar_part(pos: usize) -> (usize, usize) {
    match pos {
        0..5 => (1, pos),
        5..11 => (2, pos - 5),
        _ => (3, pos - 11),
    }
}

/// The two bytes (low, high) of character `pos` of the long name entry.
fn wchar(wep: &Winentry, pos: usize) -> (u8, u8) {
    let (part, k) = wchar_part(pos);
    let p: &[u8] = match part {
        1 => &wep.wePart1,
        2 => &wep.wePart2,
        _ => &wep.wePart3,
    };
    (p[2 * k], p[2 * k + 1])
}

/// Stores character `pos` of the long name entry as the bytes `lo`, `hi`.
fn set_wchar(wep: &mut Winentry, pos: usize, lo: u8, hi: u8) {
    let (part, k) = wchar_part(pos);
    let p: &mut [u8] = match part {
        1 => &mut wep.wePart1,
        2 => &mut wep.wePart2,
        _ => &mut wep.wePart3,
    };
    p[2 * k] = lo;
    p[2 * k + 1] = hi;
}

/// `unix2winfn(un, unlen, wep, cnt, chksum)`: create the Win95 long name directory entry
/// number `cnt` (from 1) of the name `un`. Returns the number of characters left after this
/// entry's (0 when it is the last one). Note: assumes that the filename is valid, i.e. doesn't
/// consist solely of blanks and dots.
pub fn unix2winfn(un: &[u8], wep: &mut Winentry, cnt: i32, chksum: u8) -> i32 {
    // Drop trailing blanks and dots.
    let mut unlen = un.len();
    while unlen > 0 && (un[unlen - 1] == b' ' || un[unlen - 1] == b'.') {
        unlen -= 1;
    }

    let off = (cnt - 1) as usize * WIN_CHARS;
    let mut rem = unlen as isize - off as isize;

    // Initialize winentry to some useful default.
    *wep = Winentry {
        weCnt: cnt as u8,
        wePart1: [0xff; 10],
        weAttributes: ATTR_WIN95,
        weReserved1: 0,
        weChksum: chksum,
        wePart2: [0xff; 12],
        weReserved2: [0; 2],
        wePart3: [0xff; 4],
    };

    // Now convert the filename parts.
    for pos in 0..WIN_CHARS {
        rem -= 1;
        if rem < 0 {
            set_wchar(wep, pos, 0, 0);
            wep.weCnt |= WIN_LAST;
            return 0;
        }
        set_wchar(wep, pos, un[off + pos], 0);
    }
    if rem == 0 {
        wep.weCnt |= WIN_LAST;
    }
    rem as i32
}

/// `winChkName(un, unlen, wep, chksum)`: compare our filename to the one in the Win95 entry.
/// Returns the checksum or -1 if no match.
#[allow(non_snake_case)] // the C function's name
pub fn winChkName(un: &[u8], wep: &Winentry, mut chksum: i32) -> i32 {
    // First compare checksums.
    if wep.weCnt & WIN_LAST != 0 {
        chksum = i32::from(wep.weChksum);
    } else if chksum != i32::from(wep.weChksum) {
        chksum = -1;
    }
    if chksum == -1 {
        return -1;
    }

    // Offset of this entry.
    let slot = i32::from(wep.weCnt & WIN_CNT);
    if slot == 0 {
        return -1;
    }
    let i = (slot - 1) as usize * WIN_CHARS;
    let mut unlen = un.len() as isize - i as isize;
    if unlen <= 0 {
        return -1;
    }
    let un = &un[i..];

    if wep.weCnt & WIN_LAST != 0 && unlen > WIN_CHARS as isize {
        return -1;
    }

    // Compare the name parts.
    for pos in 0..WIN_CHARS {
        let (lo, hi) = wchar(wep, pos);
        unlen -= 1;
        if unlen < 0 {
            if lo == 0 && hi == 0 {
                return chksum;
            }
            return -1;
        }
        if U2L[usize::from(lo)] != U2L[usize::from(un[pos])] || hi != 0 {
            return -1;
        }
    }
    chksum
}

/// `win2unixfn(wep, dp, chksum)`: convert Win95 filename to dirbuf. Returns the checksum or -1
/// if impossible.
pub fn win2unixfn(wep: &Winentry, dp: &mut Dirent, mut chksum: i32) -> i32 {
    let cnt = usize::from(wep.weCnt & WIN_CNT);
    if cnt > WIN_MAXLEN.div_ceil(WIN_CHARS) || cnt == 0 {
        return -1;
    }

    // First compare checksums.
    if wep.weCnt & WIN_LAST != 0 {
        chksum = i32::from(wep.weChksum);
        // This works even though d_namlen is one byte!
        dp.d_namlen = (cnt * WIN_CHARS) as u8;
    } else if chksum != i32::from(wep.weChksum) {
        chksum = -1;
    }
    if chksum == -1 {
        return -1;
    }

    // Offset of this entry; `ep` is the last byte of `d_name` a name may use.
    let mut np = (cnt - 1) * WIN_CHARS;
    let ep = WIN_MAXLEN;
    // The part (1 to 3) that holds character `WIN_MAXLEN % WIN_CHARS`: only there can the
    // name run past `ep`.
    let checked_part = wchar_part(WIN_MAXLEN % WIN_CHARS).0;

    // Convert the name parts.
    for pos in 0..WIN_CHARS {
        let (lo, hi) = wchar(wep, pos);
        dp.d_name[np] = lo;
        np += 1;
        match lo {
            0 => {
                dp.d_namlen = dp.d_namlen.wrapping_sub((WIN_CHARS - pos) as u8);
                return chksum;
            }
            b'/' => {
                dp.d_name[np - 1] = 0;
                return -1;
            }
            _ => {}
        }
        if wchar_part(pos).0 == checked_part && np > ep {
            dp.d_name[np - 1] = 0;
            return -1;
        }
        if hi != 0 {
            return -1;
        }
    }
    chksum
}

/// `winChksum(name)`: compute the checksum of a DOS filename for Win95 use.
#[allow(non_snake_case)] // the C function's name
pub fn winChksum(name: &[u8; 11]) -> u8 {
    name.iter()
        .fold(0u8, |s, &c| s.rotate_right(1).wrapping_add(c))
}

/// `winSlotCnt(un, unlen)`: determine the number of slots necessary for Win95 names.
#[allow(non_snake_case)] // the C function's name
pub fn winSlotCnt(un: &[u8]) -> i32 {
    let mut unlen = un.len();
    while unlen > 0 && (un[unlen - 1] == b' ' || un[unlen - 1] == b'.') {
        unlen -= 1;
    }
    if unlen > WIN_MAXLEN {
        return 0;
    }
    unlen.div_ceil(WIN_CHARS) as i32
}

#[cfg(test)]
mod tests;
