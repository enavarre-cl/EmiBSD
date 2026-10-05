/*	$OpenBSD: dkcksum.c,v 1.6 2014/11/19 20:28:56 miod Exp $	*/
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

//! `dkcksum()`: the checksum of a disk label.
//!
//! Upstream: sys/lib/libsa/dkcksum.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The C xors the 16-bit words up to `&lp->d_partitions[lp->d_npartitions]`, whatever
//!   `d_npartitions` says; here the walk stops at the end of the structure.

use crate::hdr::disklabel::Disklabel;

/// `dkcksum(lp)`: the xor of the label's 16-bit words, partitions included; 0 for a label
/// whose `d_checksum` is right.
pub fn dkcksum(lp: &Disklabel) -> u16 {
    let end = (148 + 16 * usize::from(lp.d_npartitions)).min(core::mem::size_of::<Disklabel>());
    lp.as_bytes()[..end]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0, |sum, w| sum ^ u16::from_ne_bytes(*w))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disklabel::getdisklabel;
    use crate::hdr::disklabel::{DISKMAGIC, dl_setpsize};

    #[test]
    fn label_checksum_round_trip() {
        let mut dl = Disklabel::zeroed();
        dl.d_magic = DISKMAGIC;
        dl.d_magic2 = DISKMAGIC;
        dl.d_secsize = 512;
        dl.d_npartitions = 16;
        dl_setpsize(&mut dl.d_partitions[0], 0x1_0000_0123);
        dl.d_checksum = dkcksum(&dl);
        assert_eq!(dkcksum(&dl), 0);

        let mut sector = [0u8; 512];
        sector[..404].copy_from_slice(&dl.as_bytes()[..404]);
        let mut got = Disklabel::zeroed();
        assert_eq!(getdisklabel(&sector, &mut got), Ok(()));
        assert_eq!(got.d_partitions[0].p_sizeh, 1);

        sector[200] ^= 1;
        assert_eq!(getdisklabel(&sector, &mut got), Err("disk label corrupted"));
        assert_eq!(getdisklabel(&[0u8; 512], &mut got), Err("no disk label"));
    }
}
