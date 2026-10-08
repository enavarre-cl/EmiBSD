/*	$OpenBSD: fat.h,v 1.13 2021/07/11 04:34:13 jsg Exp $	*/
/*	$NetBSD: fat.h,v 1.11 1997/10/17 11:23:49 ws Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (C) 1994, 1997 Wolfgang Solfrank.
 * Copyright (C) 1994, 1997 TooLs GmbH.
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
//! `<msdosfs/fat.h>`: the special cluster numbers, the FAT entry masks, the tests for the FAT
//! width of a file system and the `fatentry`/`extendfile` flags.
//!
//! Upstream: sys/msdosfs/fat.h @ 3ce1f3f79392
//!
//! ## Deviations
//! - `FAT12(pmp)`, `FAT16(pmp)`, `FAT32(pmp)` and `MSDOSFSEOF(pmp, cn)` are functions with the
//!   macros' names in lower case.
//! - The `_KERNEL` prototypes (`pcbmap`, `clusterfree`, ...) are the functions of
//!   `msdosfs_fat.rs`, where the C file defines them.

use crate::msdosfs::msdosfsmount::Msdosfsmount;

/// `MSDOSFSROOT`: cluster 0 means the root dir.
pub const MSDOSFSROOT: u32 = 0;
/// `CLUST_FREE`: cluster 0 also means a free cluster.
pub const CLUST_FREE: u32 = 0;
/// `MSDOSFSFREE`.
pub const MSDOSFSFREE: u32 = CLUST_FREE;
/// `CLUST_FIRST`: first legal cluster number.
pub const CLUST_FIRST: u32 = 2;
/// `CLUST_RSRVD`: reserved cluster range.
pub const CLUST_RSRVD: u32 = 0xfffffff6;
/// `CLUST_BAD`: a cluster with a defect.
pub const CLUST_BAD: u32 = 0xfffffff7;
/// `CLUST_EOFS`: start of eof cluster range.
pub const CLUST_EOFS: u32 = 0xfffffff8;
/// `CLUST_EOFE`: end of eof cluster range.
pub const CLUST_EOFE: u32 = 0xffffffff;
/// `CLUST_END`: bigger than any valid cluster.
pub const CLUST_END: u32 = CLUST_EOFE;

/// `FAT12_MASK`: mask for 12 bit cluster numbers.
pub const FAT12_MASK: u32 = 0x00000fff;
/// `FAT16_MASK`: mask for 16 bit cluster numbers.
pub const FAT16_MASK: u32 = 0x0000ffff;
/// `FAT32_MASK`: mask for FAT32 cluster numbers.
pub const FAT32_MASK: u32 = 0x0fffffff;

// These are the values for the function argument to the function fatentry().

/// `FAT_GET`: get a fat entry.
pub const FAT_GET: i32 = 0x0001;
/// `FAT_SET`: set a fat entry.
pub const FAT_SET: i32 = 0x0002;
/// `FAT_GET_AND_SET`.
pub const FAT_GET_AND_SET: i32 = FAT_GET | FAT_SET;

/// `DE_CLEAR`: flag to `extendfile`, zero out the blocks allocated.
pub const DE_CLEAR: i32 = 1;

/// `FAT12(pmp)`: whether the filesystem uses 12 bit fats. Microsoft Programmer's Reference
/// says if the maximum cluster number in a filesystem is greater than 4084 ((`CLUST_RSRVD` -
/// `CLUST_FIRST`) & `FAT12_MASK`) then we've got a 16 bit fat filesystem. While mounting, the
/// result of this test is stored in `pm_fatmask`.
pub fn fat12(pmp: &Msdosfsmount) -> bool {
    pmp.pm_fatmask.get() == FAT12_MASK
}

/// `FAT16(pmp)`.
pub fn fat16(pmp: &Msdosfsmount) -> bool {
    pmp.pm_fatmask.get() == FAT16_MASK
}

/// `FAT32(pmp)`.
pub fn fat32(pmp: &Msdosfsmount) -> bool {
    pmp.pm_fatmask.get() == FAT32_MASK
}

/// `MSDOSFSEOF(pmp, cn)`: whether `cn` is in the end-of-file range of the file system's FAT
/// width.
pub fn msdosfseof(pmp: &Msdosfsmount, cn: u32) -> bool {
    ((cn | !pmp.pm_fatmask.get()) & CLUST_EOFS) == CLUST_EOFS
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eof_range_per_width() {
        let pmp = Msdosfsmount::new();
        pmp.pm_fatmask.set(FAT12_MASK);
        assert!(fat12(&pmp) && !fat16(&pmp) && !fat32(&pmp));
        assert!(msdosfseof(&pmp, 0xff8));
        assert!(msdosfseof(&pmp, 0xfff));
        assert!(!msdosfseof(&pmp, 0xff7));
        pmp.pm_fatmask.set(FAT16_MASK);
        assert!(msdosfseof(&pmp, 0xfff8));
        assert!(!msdosfseof(&pmp, 0xff8));
        pmp.pm_fatmask.set(FAT32_MASK);
        assert!(fat32(&pmp));
        assert!(msdosfseof(&pmp, 0x0fff_fff8));
        assert!(msdosfseof(&pmp, CLUST_EOFE));
        assert!(!msdosfseof(&pmp, 0x0fff_fff7));
    }

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn values_match_the_c_header() {
        let defs = crate::reftest::defines("sys/msdosfs/fat.h");
        for (name, value) in [
            ("MSDOSFSROOT", i64::from(MSDOSFSROOT)),
            ("CLUST_FREE", i64::from(CLUST_FREE)),
            ("CLUST_FIRST", i64::from(CLUST_FIRST)),
            ("CLUST_RSRVD", i64::from(CLUST_RSRVD)),
            ("CLUST_BAD", i64::from(CLUST_BAD)),
            ("CLUST_EOFS", i64::from(CLUST_EOFS)),
            ("CLUST_EOFE", i64::from(CLUST_EOFE)),
            ("FAT12_MASK", i64::from(FAT12_MASK)),
            ("FAT16_MASK", i64::from(FAT16_MASK)),
            ("FAT32_MASK", i64::from(FAT32_MASK)),
            ("FAT_GET", i64::from(FAT_GET)),
            ("FAT_SET", i64::from(FAT_SET)),
            ("DE_CLEAR", i64::from(DE_CLEAR)),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }
}
/* </TESTS> */
