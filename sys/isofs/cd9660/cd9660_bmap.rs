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
 * Copyright (c) 1994
 *	The Regents of the University of California.  All rights reserved.
 *
 * This code is derived from software contributed to Berkeley
 * by Pace Willisson (pace@blitz.com).  The Rock Ridge Extension
 * Support code is derived from software contributed to Berkeley
 * by Atsushi Murai (amurai@spec.co.jp).
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
 *	@(#)cd9660_bmap.c	8.4 (Berkeley) 12/5/94
 */
/* </LICENSES> */

/* <CODE> */
//! `cd9660_bmap`: the logical to physical block mapping of an ISO 9660 file, which is one
//! contiguous extent on the disc.
//!
//! Upstream: sys/isofs/cd9660/cd9660_bmap.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The optional results (`a_vpp`, `a_bnp`, `a_runp`) are `Option<&mut>`s, the C's NULL
//!   being `None` (`sys/vnode.rs`); the block arithmetic is in `i64` (`daddr_t`).

use crate::isofs::cd9660::cd9660_node::vtoi;
use crate::sys::errno::Errno;
use crate::sys::param::{DEV_BSHIFT, MAXBSIZE};
use crate::sys::vnode::VopBmapArgs;

/// `cd9660_bmap` (`vop_bmap`): converts a the logical block number of a file to its
/// physical block number on the disk. The conversion is done by using the logical block
/// number to index into the data block (extent) for the file.
pub fn cd9660_bmap(ap: &mut VopBmapArgs<'_>) -> Result<(), Errno> {
    let ip = vtoi(ap.a_vp);
    let lblkno = ap.a_bn;

    // Check for underlying vnode requests and ensure that logical to physical mapping is
    // requested.
    if let Some(vpp) = ap.a_vpp.as_deref_mut() {
        *vpp = ip.i_devvp.get();
    }
    let Some(bnp) = ap.a_bnp.as_deref_mut() else {
        return Ok(());
    };

    // Compute the requested block number
    let bshift = ip.mnt().im_bshift;
    *bnp = (ip.iso_start.get() as i64).wrapping_add(lblkno) << (bshift - DEV_BSHIFT as i32);

    // Determine maximum number of readahead blocks following the requested block.
    if let Some(runp) = ap.a_runp.as_deref_mut() {
        let nblk = (ip.i_size.get() >> bshift) as i64 - (lblkno + 1);
        let max = (MAXBSIZE >> bshift) as i64;
        *runp = if nblk <= 0 {
            0
        } else if nblk >= max {
            (max - 1) as i32
        } else {
            nblk as i32
        };
    }

    Ok(())
}
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::isofs::cd9660::cd9660_extern::ISO_FTYPE_RRIP;
    use crate::isofs::cd9660::iso::tests::{test_mnt, test_node};
    use crate::sys::vnode::Vnode;

    /// `VOP_BMAP` of `lbn`: the device vnode, the device block and the run.
    fn bmap(vp: &'static Vnode, lbn: i64) -> (Option<&'static Vnode>, i64, i32) {
        let (mut vpp, mut bn, mut run) = (None, 0, 0);
        let mut a = VopBmapArgs {
            a_vp: vp,
            a_bn: lbn,
            a_vpp: Some(&mut vpp),
            a_bnp: Some(&mut bn),
            a_runp: Some(&mut run),
        };
        cd9660_bmap(&mut a).unwrap();
        (vpp, bn, run)
    }

    #[test]
    fn blocks_map_into_the_extent() {
        let imp = test_mnt(ISO_FTYPE_RRIP);
        let (ip, vp) = test_node(imp);
        ip.i_devvp.set(Some(imp.im_devvp));
        ip.iso_start.set(23);
        ip.i_size.set(12);
        let (dev, bn, run) = bmap(vp, 0);
        assert!(dev.is_some_and(|d| core::ptr::eq(d, imp.im_devvp)));
        assert_eq!((bn, run), (23 * 4, 0));
        ip.i_size.set(10 * 2048);
        let (_, bn, run) = bmap(vp, 2);
        assert_eq!((bn, run), (25 * 4, 7));
        ip.i_size.set(100 * 2048);
        assert_eq!(bmap(vp, 0).2, 31);
    }
}
/* </TESTS> */
