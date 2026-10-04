/*	$OpenBSD: disksubr.c,v 1.79 2025/09/28 22:34:19 krw Exp $	*/
/*	$NetBSD: disksubr.c,v 1.21 1996/05/03 19:42:03 christos Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 1996 Theo de Raadt
 * Copyright (c) 1982, 1986, 1988 Regents of the University of California.
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
 */
/* </LICENSES> */

//! amd64's disk label I/O: `arch/amd64/amd64/disksubr.c` (`readdisklabel`,
//! `writedisklabel`), reached through `machine::disklabel`.
//!
//! Upstream: sys/arch/amd64/amd64/disksubr.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `spoofonly` is a `bool`; the functions return `Result` (`docs/C_TO_RUST.md`).
//! - The BIOS geometry `readdisklabel` honours (`bios_getdiskinfo`, `<machine/biosvar.h>`)
//!   does not exist: the kernel boots through Limine on UEFI, without `boot(8)`'s
//!   `bios_diskinfo` list (`skipped: replaced-by-limine`), so the driver's geometry stands,
//!   as on arm64.

use core::sync::atomic::Ordering;

use crate::kern::subr_disk::{DISK_CHANGE, initdisklabel, readdisksector, readdoslabel};
use crate::kern::vfs_bio::{biowait, brelse, geteblk};
use crate::sys::buf::{B_BUSY, B_DONE, B_INVAL, B_RAW, B_READ, B_WRITE, Buf};
use crate::sys::conf::DevTypeStrategy;
use crate::sys::disklabel::{
    DISKLABEL_SIZE, DOS_LABELSECTOR, Disklabel, dl_blkoffset, dl_blktosec,
};
use crate::sys::errno::Errno;
use crate::sys::types::{Daddr, Dev};

/// Attempt to read a disk label from a device using the indicated strategy routine. The
/// label must be partly set up before this: secpercyl, secsize and anything required for a
/// block i/o read operation in the driver's strategy/start routines must be filled in before
/// calling us.
///
/// If dos partition table requested, attempt to load it and find disklabel inside a DOS
/// partition.
///
/// We would like to check if each MBR has a valid DOSMBR_SIGNATURE, but we cannot because
/// it doesn't always exist. So.. we assume the MBR is valid.
pub fn readdisklabel(
    dev: Dev,
    strat: DevTypeStrategy,
    lp: &mut Disklabel,
    spoofonly: bool,
) -> Result<(), Errno> {
    let mut bp: Option<&'static Buf> = None;
    let result = (|| {
        initdisklabel(lp)?;

        // Look for any BIOS geometry information we should honour: none under Limine
        // (see the module's deviations).

        // get a buffer and initialize it
        let b = geteblk(lp.d_secsize as usize);
        bp = Some(b);
        b.b_dev.set(dev);

        // Each spoof is tried in turn while nothing has matched (the C's `goto done`).
        #[cfg_attr(not(any(feature = "cd9660", feature = "udf")), allow(unused_mut))]
        let mut error = readdoslabel(b, strat, lp, None, spoofonly);
        #[cfg(feature = "cd9660")]
        if error.is_err() {
            error = crate::isofs::cd9660::cd9660_vfsops::iso_disklabelspoof(dev, strat, lp);
        }
        #[cfg(feature = "udf")]
        if error.is_err() {
            error = crate::isofs::udf::udf_subr::udf_disklabelspoof(dev, strat, lp);
        }
        error
    })();

    if let Some(bp) = bp {
        bp.set(B_INVAL);
        brelse(bp);
    }
    DISK_CHANGE.store(1, Ordering::Relaxed);
    result
}

/// Write disk label back to device after modification.
pub fn writedisklabel(dev: Dev, strat: DevTypeStrategy, lp: &mut Disklabel) -> Result<(), Errno> {
    // get a buffer and initialize it
    let bp = geteblk(lp.d_secsize as usize);
    bp.b_dev.set(dev);

    let result = (|| {
        let mut partoff: Daddr = -1;
        if readdoslabel(bp, strat, lp, Some(&mut partoff), true).is_err() {
            return Err(Errno::EIO);
        }

        // Read it in, slap the new label in, and write it back out
        let blk = (partoff + DOS_LABELSECTOR) as u64;
        readdisksector(bp, strat, lp, dl_blktosec(lp, blk))?;
        let offset = dl_blkoffset(lp, blk) as usize;

        // SAFETY: the buffer is ours (busy since `geteblk`), mapped for `b_bcount` bytes.
        let data = unsafe { bp.data() };
        let Some(dst) = data.get_mut(offset..) else {
            return Err(Errno::EIO);
        };
        let n = dst.len().min(DISKLABEL_SIZE);
        dst[..n].copy_from_slice(&lp.as_bytes()[..n]);
        bp.clr(B_READ | B_WRITE | B_DONE);
        bp.set(B_BUSY | B_WRITE | B_RAW);
        strat(bp);
        biowait(bp)
    })();

    bp.set(B_INVAL);
    brelse(bp);
    DISK_CHANGE.store(1, Ordering::Relaxed);
    result
}
