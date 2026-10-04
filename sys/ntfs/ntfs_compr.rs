/*	$OpenBSD: ntfs_compr.h,v 1.2 2003/05/20 03:23:11 mickey Exp $	*/
/*	$NetBSD: ntfs_compr.h,v 1.1 2002/12/23 17:38:32 jdolecek Exp $	*/
/*	$OpenBSD: ntfs_compr.c,v 1.7 2013/11/24 16:02:30 jsing Exp $	*/
/*	$NetBSD: ntfs_compr.c,v 1.1 2002/12/23 17:38:31 jdolecek Exp $	*/
/* <LICENSES> */
/*-
 * Copyright (c) 1998, 1999 Semen Ustimenko
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	Id: ntfs_compr.h,v 1.4 1999/05/12 09:42:55 semenu Exp
 */
/*-
 * Copyright (c) 1998, 1999 Semen Ustimenko
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
 *
 * THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 *	Id: ntfs_compr.c,v 1.4 1999/05/12 09:42:54 semenu Exp
 */
/* </LICENSES> */

//! NTFS compression (LZNT1): a compression unit is `NTFS_COMPUNIT_CL` clusters, cut into
//! blocks of `NTFS_COMPBLOCK_SIZE` bytes, each stored either as is or as a stream of tag
//! bytes, literals and back references whose split between offset and length bits widens as
//! the block's output grows.
//!
//! Upstream: sys/ntfs/ntfs_compr.h @ 3ce1f3f79392, sys/ntfs/ntfs_compr.c @ 3ce1f3f79392
//!
//! ## Deviations
//! - The header and the file share this module, as `.h`/`.c` pairs do (`siphash.rs`).
//! - `ntfs_uncompblock(buf, cbuf)` takes the output block as a slice of at least
//!   `NTFS_COMPBLOCK_SIZE` bytes and the compressed bytes as the slice from the block's
//!   header on; `GET_UINT16` reads in the machine's byte order, and a byte past the end of
//!   `cbuf` reads as 0 (the C reads past its buffer). A back reference that reaches before the
//!   start of the block reads 0 (the C reads before `buf`). `ntfs_uncompunit` takes the two
//!   unit buffers as slices.
//! - The `DPRINTF`s (`NTFS_DEBUG`, off) are left out.

use crate::ntfs::ntfs::Ntfsmount;
use crate::sys::errno::Errno;

/// `NTFS_COMPBLOCK_SIZE`.
pub const NTFS_COMPBLOCK_SIZE: usize = 0x1000;
/// `NTFS_COMPUNIT_CL`: the clusters of a compression unit.
pub const NTFS_COMPUNIT_CL: u64 = 16;

/// `GET_UINT16(cbuf + off)`, 0 for bytes past the end.
fn get_uint16(cbuf: &[u8], off: usize) -> u32 {
    let b = |i: usize| cbuf.get(i).copied().unwrap_or(0);
    u32::from(u16::from_ne_bytes([b(off), b(off + 1)]))
}

/// `ntfs_uncompblock(buf, cbuf)`: decompress the block at the start of `cbuf` into `buf`
/// (`NTFS_COMPBLOCK_SIZE` bytes). Returns the size of the compressed block (its header
/// included).
pub fn ntfs_uncompblock(buf: &mut [u8], cbuf: &[u8]) -> usize {
    let len = (get_uint16(cbuf, 0) & 0xFFF) as usize;
    let cb = |i: usize| cbuf.get(i).copied().unwrap_or(0);

    if get_uint16(cbuf, 0) & 0x8000 == 0 {
        // A block that is stored as is: len + 1 should be NTFS_COMPBLOCK_SIZE.
        for (i, b) in buf[..=len].iter_mut().enumerate() {
            *b = cb(2 + i);
        }
        buf[len + 1..NTFS_COMPBLOCK_SIZE].fill(0);
        return len + 3;
    }
    let mut cpos = 2usize;
    let mut pos = 0usize;
    while cpos < len + 3 && pos < NTFS_COMPBLOCK_SIZE {
        let mut ctag = u32::from(cb(cpos));
        cpos += 1;
        let mut i = 0;
        while i < 8 && pos < NTFS_COMPBLOCK_SIZE {
            if ctag & 1 != 0 {
                let mut lmask: u32 = 0xFFF;
                let mut dshift: u32 = 12;
                let mut j = pos as i64 - 1;
                while j >= 0x10 {
                    dshift -= 1;
                    lmask >>= 1;
                    j >>= 1;
                }
                let boff = -1 - (get_uint16(cbuf, cpos) >> dshift) as i64;
                let blen = 3 + (get_uint16(cbuf, cpos) & lmask) as usize;
                let mut j = 0;
                while j < blen && pos < NTFS_COMPBLOCK_SIZE {
                    let from = pos as i64 + boff;
                    buf[pos] = if from >= 0 { buf[from as usize] } else { 0 };
                    pos += 1;
                    j += 1;
                }
                cpos += 2;
            } else {
                buf[pos] = cb(cpos);
                pos += 1;
                cpos += 1;
            }
            ctag >>= 1;
            i += 1;
        }
    }
    len + 3
}

/// `ntfs_uncompunit(ntmp, uup, cup)`: decompress the compression unit `cup` into `uup`, both
/// `ntfs_cntob(NTFS_COMPUNIT_CL)` bytes.
pub fn ntfs_uncompunit(ntmp: &Ntfsmount, uup: &mut [u8], cup: &[u8]) -> Result<(), Errno> {
    let unit = usize::try_from(ntmp.ntfs_cntob(NTFS_COMPUNIT_CL)).unwrap_or(0);
    let mut off = 0usize;
    let mut i = 0usize;

    while i * NTFS_COMPBLOCK_SIZE < unit {
        let new = ntfs_uncompblock(
            &mut uup[i * NTFS_COMPBLOCK_SIZE..],
            cup.get(off..).unwrap_or(&[]),
        );
        if new == 0 {
            return Err(Errno::EINVAL);
        }
        off += new;
        i += 1;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
