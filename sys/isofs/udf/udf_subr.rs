/*	$OpenBSD: udf_subr.c,v 1.27 2024/04/13 23:44:11 jsg Exp $	*/
/* <LICENSES> */
/*
 * Copyright (c) 2006, Miodrag Vallat
 * Copyright (c) 2006, Pedro Martelletto
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR ``AS IS'' AND ANY EXPRESS OR
 * IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
 * WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
 * DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY DIRECT,
 * INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
 * (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
 * SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN
 * ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
/* </LICENSES> */

/* <CODE> */
//! UDF helpers: CS0 names to Unicode, the disk label a UDF medium is given
//! (`udf_disklabelspoof`, called by each arch's `readdisklabel`), and the virtual allocation
//! table (VAT) of write-once media.
//!
//! Upstream: sys/isofs/udf/udf_subr.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - `udf_rawnametounicode` returns `Err(EINVAL)` where the C returns -1, and the number of
//!   characters otherwise; it also fails when the name does not fit in `cs0string` or in
//!   `transname` (the C trusts its callers). The bytes of the name are unsigned on both
//!   archs (as the C's `char` is on arm64); on amd64 the C's signed `char` sign-extends a
//!   byte >= 0x80, so such a byte of an 8-bit name came out as `?`.
//! - `udf_vat_get` keeps a copy of the VAT's unode with its own copy of the file entry
//!   (`malloc(M_UDFMOUNT)` and `malloc(M_UDFFENTRY)`, both freed by `udf_unmount`); the C's
//!   structure copy shares `u_fentry` with the cached unode, which `udf_reclaim` frees.
//! - `udf_vat_free` (crate-private) gives that copy back, for `udf_unmount` and the mount's
//!   error path (the C's `free(ump->um_vat, M_UDFMOUNT, 0)`).
//! - `udf_vat_read` releases the buffer only when there is one: a VAT recorded in its file
//!   entry has none (the C would `brelse(NULL)`).

use core::ptr::NonNull;

use libkern::strlcpy;

use crate::isofs::udf::ecma167_udf::{
    AnchorVdp, DescTag, Packed, PriVolDesc, TAGID_ANCHOR, TAGID_PRI_VOL,
};
use crate::isofs::udf::udf::{UDF_MNT_FIND_VAT, UDF_MNT_USES_VAT, Umount, Unicode, Unode, VTOU};
use crate::isofs::udf::udf_vfsops::{udf_checktag, udf_vget};
use crate::isofs::udf::udf_vnops::{udf_data, udf_readatoffset, udf_transname};
use crate::kern::kern_malloc::malloc;
use crate::kern::subr_disk::dkcksum;
use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{biowait, brelse, geteblk};
use crate::kern::vfs_subr::vput;
use crate::sys::buf::{B_BUSY, B_DONE, B_INVAL, B_RAW, B_READ, B_WRITE, Buf};
use crate::sys::conf::DevTypeStrategy;
use crate::sys::disklabel::{
    DISKMAGIC, Disklabel, FS_UDF, MAXPARTITIONS, RAW_PART, dl_getdsize, dl_setpoffset, dl_setpsize,
};
use crate::sys::endian::{letoh32, letoh64};
use crate::sys::errno::Errno;
use crate::sys::malloc::{M_UDFFENTRY, M_UDFMOUNT, M_WAITOK};
use crate::sys::param::btodb;
use crate::sys::types::{Daddr, Dev, Ino, Off};

/// Convert a CS0 dstring to a 16-bit Unicode string. Returns the length of the Unicode
/// string, in unicode characters (not bytes!). Note that the transname destination buffer is
/// expected to be large enough to hold the result, and will not be terminated in any way.
pub fn udf_rawnametounicode(
    len: u32,
    cs0string: &[u8],
    transname: &mut [Unicode],
) -> Result<usize, Errno> {
    let Some(len) = len.checked_sub(1) else {
        return Err(Errno::EINVAL);
    };
    let len = len as usize;
    let Some((&kind, name)) = cs0string.split_first() else {
        return Err(Errno::EINVAL);
    };
    let Some(name) = name.get(..len) else {
        return Err(Errno::EINVAL);
    };

    match kind {
        8 => {
            // bytes string
            let Some(out) = transname.get_mut(..len) else {
                return Err(Errno::EINVAL);
            };
            for (t, &c) in out.iter_mut().zip(name) {
                *t = Unicode::from(c);
            }
            Ok(len)
        }
        16 => {
            // 16 bit unicode string
            if len & 1 != 0 {
                return Err(Errno::EINVAL);
            }
            let n = len >> 1;
            let Some(out) = transname.get_mut(..n) else {
                return Err(Errno::EINVAL);
            };
            for (t, c) in out.iter_mut().zip(name.as_chunks::<2>().0) {
                *t = (Unicode::from(c[0]) << 8) | Unicode::from(c[1]);
            }
            Ok(n)
        }
        _ => Err(Errno::EINVAL),
    }
}

/// Read the logical block `sector` (of `bsize` bytes) of the medium into `bp` through the
/// driver's strategy, as `udf_disklabelspoof` does twice.
fn spoof_read(
    bp: &'static Buf,
    strat: DevTypeStrategy,
    secpercyl: u32,
    sector: u32,
    bsize: usize,
) -> Result<(), Errno> {
    bp.b_blkno
        .set(Daddr::from(sector).wrapping_mul(btodb(bsize) as Daddr));
    bp.b_bcount.set(bsize as i64);
    bp.clr(B_READ | B_WRITE | B_DONE);
    bp.set(B_BUSY | B_READ | B_RAW);
    if secpercyl != 0 {
        bp.b_resid
            .set((bp.b_blkno.get() / Daddr::from(secpercyl)) as usize);
    }

    strat(bp);
    biowait(bp)
}

/// Do a lazy probe on the underlying media to check if it's a UDF volume, in which case we
/// fake a disk label for it.
pub fn udf_disklabelspoof(
    dev: Dev,
    strat: DevTypeStrategy,
    lp: &mut Disklabel,
) -> Result<(), Errno> {
    let mut vid = [0u8; 32];
    let bsize = 2048usize;
    let secpercyl = lp.d_secpercyl;

    // Get a buffer to work with.
    let bp = geteblk(bsize);
    bp.b_dev.set(dev);

    let error = 'out: {
        // Look for an Anchor Volume Descriptor at sector 256.
        if spoof_read(bp, strat, secpercyl, 256, bsize).is_err() {
            break 'out Err(Errno::EINVAL);
        }

        let avdp = {
            // SAFETY: the buffer is ours (busy since `geteblk`) and mapped; the slice dies
            // in this block.
            let data = unsafe { bp.data() };
            let Some(tag) = DescTag::at(data, 0) else {
                break 'out Err(Errno::EINVAL);
            };
            if udf_checktag(tag, TAGID_ANCHOR).is_err() {
                break 'out Err(Errno::EINVAL);
            }
            match AnchorVdp::at(data, 0) {
                Some(avdp) => *avdp,
                None => break 'out Err(Errno::EINVAL),
            }
        };
        let mvds_start = letoh32(avdp.main_vds_ex.loc);
        let mvds_end =
            mvds_start.wrapping_add(letoh32(avdp.main_vds_ex.len).wrapping_sub(1) / bsize as u32);

        // Then try to find a reference to a Primary Volume Descriptor.
        let mut sector = mvds_start;
        let mut vol_id = None;
        while sector < mvds_end {
            if spoof_read(bp, strat, secpercyl, sector, bsize).is_err() {
                break 'out Err(Errno::EINVAL);
            }

            // SAFETY: as above.
            let data = unsafe { bp.data() };
            if let Some(pvd) = PriVolDesc::at(data, 0)
                && udf_checktag(&pvd.tag, TAGID_PRI_VOL).is_ok()
            {
                vol_id = Some(pvd.vol_id);
                break;
            }
            sector += 1;
        }

        // If we couldn't find a reference, bail out.
        let Some(vol_id) = vol_id else {
            break 'out Err(Errno::EINVAL);
        };

        // Okay, it's a UDF volume. Spoof a disk label for it.
        if udf_transname(&vol_id, &mut vid, vol_id.len() as i32 - 1, None) != 0 {
            strlcpy(&mut lp.d_typename, &vid);
        }

        for p in lp.d_partitions.iter_mut().take(MAXPARTITIONS) {
            dl_setpsize(p, 0);
            dl_setpoffset(p, 0);
        }

        // Fake two partitions, 'a' and 'c'.
        let dsize = dl_getdsize(lp);
        dl_setpsize(&mut lp.d_partitions[0], dsize);
        lp.d_partitions[0].p_fstype = FS_UDF;
        dl_setpsize(&mut lp.d_partitions[RAW_PART as usize], dsize);
        lp.d_partitions[RAW_PART as usize].p_fstype = FS_UDF;
        lp.d_npartitions = MAXPARTITIONS as u16;
        lp.d_version = 1;

        lp.d_magic = DISKMAGIC;
        lp.d_magic2 = DISKMAGIC;
        lp.d_checksum = dkcksum(lp);

        Ok(())
    };

    bp.set(B_INVAL);
    brelse(bp);

    error
}

/// Get a vnode for the Virtual Allocation Table (VAT).
pub fn udf_vat_get(ump: &'static Umount, lb: u32) -> Result<(), Errno> {
    let ino = lb.wrapping_sub(ump.um_start.get()).wrapping_sub(3);
    let vp = udf_vget(ump.mountp(), Ino::from(ino))?;

    let up = VTOU(vp);
    up.set_u_vatlen((letoh64(up.fentry().inf_len).wrapping_sub(36) >> 2) as i64);

    // ump->um_vat = malloc(sizeof(struct unode)); *ump->um_vat = *up;
    let Some(mem) = malloc(size_of::<Unode>(), M_UDFMOUNT, M_WAITOK) else {
        panic(format_args!("udf_vat_get: no memory"));
    };
    let vat = mem.cast::<Unode>();
    // SAFETY: a fresh `malloc` of a unode's size (malloc aligns to its bucket, at least 16),
    // written once here; `udf_unmount` (or the mount's error path) frees it.
    let vat: &'static Unode = unsafe {
        vat.as_ptr().write(Unode::new());
        vat.as_ref()
    };
    vat.u_vnode.set(up.u_vnode.get());
    vat.u_devvp.set(up.u_devvp.get());
    vat.u_ump.set(up.u_ump.get());
    vat.u_dev.set(up.u_dev.get());
    vat.u_ino.set(up.u_ino.get());
    vat.un_u.set(up.un_u.get());
    let fentry = up.fentry_bytes();
    if !fentry.is_empty() {
        let Some(copy) = malloc(fentry.len(), M_UDFFENTRY, M_WAITOK) else {
            panic(format_args!("udf_vat_get: no memory"));
        };
        // SAFETY: `copy` is a fresh allocation of `fentry.len()` bytes, which cannot overlap
        // the node's own `u_fentry`.
        unsafe {
            core::ptr::copy_nonoverlapping(fentry.as_ptr(), copy.as_ptr(), fentry.len());
        }
        vat.u_fentry.set(Some(copy));
        vat.u_fentry_len.set(fentry.len());
    }
    ump.um_vat.set(Some(vat));

    ump.um_flags
        .set((ump.um_flags.get() & !UDF_MNT_FIND_VAT) | UDF_MNT_USES_VAT);

    vput(vp);

    Ok(())
}

/// Look up a sector in the VAT.
pub fn udf_vat_map(ump: &Umount, sector: &mut u32) -> Result<(), Errno> {
    // If there's no VAT, then it's easy
    if ump.um_flags.get() & UDF_MNT_USES_VAT == 0 {
        *sector = sector.wrapping_add(ump.um_start.get());
        return Ok(());
    }

    // Sanity check the given sector
    let Some(vat) = ump.um_vat.get() else {
        return Err(Errno::EINVAL);
    };
    if i64::from(*sector) >= vat.u_vatlen() {
        return Err(Errno::EINVAL);
    }

    udf_vat_read(ump, sector)
}

/// Read from the VAT.
pub fn udf_vat_read(ump: &Umount, sector: &mut u32) -> Result<(), Errno> {
    let Some(vat) = ump.um_vat.get() else {
        return Err(Errno::EINVAL);
    };
    let mut size = 4;
    let mut bp = None;

    // Note that we rely on the buffer cache to keep frequently accessed buffers around to
    // avoid reading them from the disk all the time.
    let release = |bp: Option<&'static Buf>| {
        if let Some(bp) = bp {
            brelse(bp);
        }
    };
    let data = match udf_readatoffset(vat, &mut size, Off::from(*sector) << 2, &mut bp) {
        Ok(data) => data,
        Err(e) => {
            release(bp);
            return Err(e);
        }
    };

    // Make sure we read at least a whole entry
    if size < 4 {
        release(bp);
        return Err(Errno::EINVAL);
    }

    // Map the sector
    let entry = {
        // SAFETY: `bp` is the buffer `udf_readatoffset` returned, busy for us until the
        // release below, and no other slice of it is alive.
        let d = unsafe { udf_data(vat, bp, data, size) };
        match d.get(..4) {
            Some(e) => u32::from_ne_bytes([e[0], e[1], e[2], e[3]]),
            None => {
                release(bp);
                return Err(Errno::EINVAL);
            }
        }
    };
    *sector = letoh32(entry).wrapping_add(ump.um_start.get());

    release(bp);

    Ok(())
}

/// Give back the VAT copy `udf_vat_get` made (the C's `free(ump->um_vat, M_UDFMOUNT, 0)`).
pub(crate) fn udf_vat_free(ump: &Umount) {
    let Some(vat) = ump.um_vat.take() else {
        return;
    };
    if let Some(fe) = vat.u_fentry.take() {
        crate::kern::kern_malloc::free(fe, M_UDFFENTRY, vat.u_fentry_len.get());
    }
    crate::kern::kern_malloc::free(
        NonNull::from(vat).cast::<u8>(),
        M_UDFMOUNT,
        size_of::<Unode>(),
    );
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cs0_names_become_unicode() {
        let mut t = [0u16; 8];
        assert_eq!(udf_rawnametounicode(5, b"\x08abcd", &mut t), Ok(4));
        assert_eq!(&t[..4], &[0x61, 0x62, 0x63, 0x64]);
        assert_eq!(udf_rawnametounicode(2, b"\x08\xe9", &mut t), Ok(1));
        assert_eq!(t[0], 0xe9);
        assert_eq!(udf_rawnametounicode(5, b"\x10\x00A\x26\x3a", &mut t), Ok(2));
        assert_eq!(&t[..2], &[0x41, 0x263a]);
        // Odd 16-bit length, unknown compression id, empty, longer than the buffer.
        assert_eq!(
            udf_rawnametounicode(4, b"\x10abc", &mut t),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            udf_rawnametounicode(2, b"\x07a", &mut t),
            Err(Errno::EINVAL)
        );
        assert_eq!(udf_rawnametounicode(0, b"", &mut t), Err(Errno::EINVAL));
        assert_eq!(
            udf_rawnametounicode(10, b"\x08abcdefghi", &mut t),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            udf_rawnametounicode(9, b"\x08ab", &mut t),
            Err(Errno::EINVAL)
        );
    }
}
/* </TESTS> */
