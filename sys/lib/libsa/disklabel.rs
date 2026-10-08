/*	$OpenBSD: disklabel.c,v 1.7 2025/11/20 14:57:39 krw Exp $	*/
/*	$NetBSD: disklabel.c,v 1.3 1994/10/26 05:44:42 cgd Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1993
 *	The Regents of the University of California.  All rights reserved.
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
 *	@(#)disklabel.c	8.1 (Berkeley) 6/11/93
 */
/* </LICENSES> */

/* <CODE> */
//! `getdisklabel()`: find and check the disk label in a sector.
//!
//! Upstream: sys/lib/libsa/disklabel.c @ 3ce1f3f79392
//!
//! Only the old smaller "skinny" label, which has 16 partitions, is read: the C carves
//! `struct disklabel` with `offsetof(struct disklabel, d_partitions[MAXPARTITIONS16])`; the
//! "fat" label with 52 partitions is left for later, as in C.
//!
//! ## Deviations
//! - The message is an `Err(&'static str)` (the C returns it, or NULL for success). The
//!   candidate labels are copied out of the sector before they are checked, as the sector
//!   bytes have no alignment for Rust; the C reads them in place.

use crate::dkcksum::dkcksum;
use crate::hdr::disklabel::{DISKMAGIC, Disklabel, MAXPARTITIONS16};
use crate::hdr::param::DEV_BSIZE;

/// `offsetof(struct disklabel, d_partitions[MAXPARTITIONS16])`: the skinny label's size.
pub const SKINNY_LABEL_SIZE: usize = 148 + 16 * MAXPARTITIONS16;

/// `getdisklabel(buf, lp)`: copy the label found in the [`DEV_BSIZE`] bytes of `buf` into
/// `lp` (its skinny part), or say why there is none.
pub fn getdisklabel(buf: &[u8], lp: &mut Disklabel) -> Result<(), &'static str> {
    let lpsz = SKINNY_LABEL_SIZE;
    let mut msg = None;
    let step = core::mem::size_of::<u64>();
    let end = DEV_BSIZE.min(buf.len()).saturating_sub(lpsz);

    let mut off = 0;
    while off <= end && off + lpsz <= buf.len() {
        let mut dl = Disklabel::zeroed();
        dl.as_bytes_mut()[..lpsz].copy_from_slice(&buf[off..off + lpsz]);
        if dl.d_magic != DISKMAGIC || dl.d_magic2 != DISKMAGIC {
            if msg.is_none() {
                msg = Some("no disk label");
            }
        } else if usize::from(dl.d_npartitions) > MAXPARTITIONS16 || dkcksum(&dl) != 0 {
            msg = Some("disk label corrupted");
        } else {
            lp.as_bytes_mut()[..lpsz].copy_from_slice(&dl.as_bytes()[..lpsz]);
            msg = None;
            break;
        }
        off += step;
    }
    match msg {
        Some(m) => Err(m),
        None => Ok(()),
    }
}
/* </CODE> */
