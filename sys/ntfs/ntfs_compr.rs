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

/* <CODE> */
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
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
pub(crate) mod tests {
    // Host tests for LZNT1 decompression: a small compressor (greedy, the encoding of the C's
    // decoder: the split of a back reference's 16 bits between displacement and length widens
    // with the output position), stored and compressed blocks, back references that overlap
    // their output, and whole compression units.

    use std::vec;
    use std::vec::Vec;

    use super::*;
    use crate::ntfs::ntfs::Bootfile;

    /// The `dshift` and `lmask` the decoder uses at output position `pos`.
    fn split(pos: usize) -> (u32, u32) {
        let mut lmask: u32 = 0xFFF;
        let mut dshift: u32 = 12;
        let mut j = pos as i64 - 1;
        while j >= 0x10 {
            dshift -= 1;
            lmask >>= 1;
            j >>= 1;
        }
        (dshift, lmask)
    }

    /// LZNT1-compress one block of at most `NTFS_COMPBLOCK_SIZE` bytes: header, then groups of a
    /// tag byte and eight literals or back references.
    pub(crate) fn compress_block(src: &[u8]) -> Vec<u8> {
        assert!(src.len() <= NTFS_COMPBLOCK_SIZE);
        let mut out = vec![0u8, 0u8];
        let mut pos = 0;
        while pos < src.len() {
            let tagpos = out.len();
            out.push(0);
            let mut tag = 0u8;
            for bit in 0..8 {
                if pos >= src.len() {
                    break;
                }
                let (dshift, lmask) = split(pos);
                let maxdisp = (1usize << (16 - dshift)).min(pos);
                let maxlen = (lmask as usize + 3).min(src.len() - pos);
                let (mut best, mut bestdisp) = (0, 0);
                for disp in 1..=maxdisp {
                    let mut l = 0;
                    while l < maxlen && src[pos + l] == src[pos + l - disp] {
                        l += 1;
                    }
                    if l > best {
                        best = l;
                        bestdisp = disp;
                    }
                }
                if best >= 3 {
                    let v = (((bestdisp - 1) as u32) << dshift) | (best as u32 - 3);
                    out.extend_from_slice(&(v as u16).to_le_bytes());
                    tag |= 1 << bit;
                    pos += best;
                } else {
                    out.push(src[pos]);
                    pos += 1;
                }
            }
            out[tagpos] = tag;
        }
        let hdr = 0xB000u16 | (out.len() - 3) as u16;
        out[..2].copy_from_slice(&hdr.to_le_bytes());
        out
    }

    /// A block stored as is.
    pub(crate) fn stored_block(src: &[u8]) -> Vec<u8> {
        assert_eq!(src.len(), NTFS_COMPBLOCK_SIZE);
        let mut out = 0x3FFFu16.to_le_bytes().to_vec();
        out.extend_from_slice(src);
        out
    }

    /// Text that compresses well, with some variety.
    pub(crate) fn sample(n: usize, seed: u32) -> Vec<u8> {
        let words: [&[u8]; 6] = [
            b"ntfs ",
            b"openbsd ",
            b"cluster ",
            b"run ",
            b"lznt1 ",
            b"\n",
        ];
        let mut x = seed;
        let mut out = Vec::new();
        while out.len() < n {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            out.extend_from_slice(words[(x >> 16) as usize % words.len()]);
        }
        out.truncate(n);
        out
    }

    #[test]
    fn a_stored_block_is_copied() {
        let src: Vec<u8> = (0..NTFS_COMPBLOCK_SIZE).map(|i| (i * 7) as u8).collect();
        let cb = stored_block(&src);
        let mut out = vec![0xAAu8; NTFS_COMPBLOCK_SIZE];
        assert_eq!(ntfs_uncompblock(&mut out, &cb), NTFS_COMPBLOCK_SIZE + 2);
        assert_eq!(out, src);
    }

    #[test]
    fn a_short_stored_block_is_zero_filled() {
        // Header 0x0004: not compressed, five bytes.
        let cb = [0x04, 0x00, 1, 2, 3, 4, 5];
        let mut out = vec![0xAAu8; NTFS_COMPBLOCK_SIZE];
        assert_eq!(ntfs_uncompblock(&mut out, &cb), 7);
        assert_eq!(&out[..5], &[1, 2, 3, 4, 5]);
        assert!(out[5..].iter().all(|&b| b == 0));
    }

    #[test]
    fn compressed_blocks_round_trip() {
        for (n, seed) in [
            (NTFS_COMPBLOCK_SIZE, 1),
            (1000, 2),
            (17, 3),
            (NTFS_COMPBLOCK_SIZE, 4),
        ] {
            let src = sample(n, seed);
            let cb = compress_block(&src);
            assert!(cb.len() < n / 2 || n < 100, "{} -> {}", n, cb.len());
            let mut out = vec![0xAAu8; NTFS_COMPBLOCK_SIZE];
            assert_eq!(ntfs_uncompblock(&mut out, &cb), cb.len());
            assert_eq!(&out[..n], &src[..]);
        }
    }

    #[test]
    fn a_back_reference_may_overlap_its_output() {
        // "ab" then a reference to 1..=2 bytes back for 10 bytes: "abababababab".
        let v: u16 = (1 << 12) | (10 - 3);
        let mut cb = vec![0, 0, 0b100, b'a', b'b'];
        cb.extend_from_slice(&v.to_le_bytes());
        let hdr = 0xB000u16 | (cb.len() - 3) as u16;
        cb[..2].copy_from_slice(&hdr.to_le_bytes());
        let mut out = vec![0u8; NTFS_COMPBLOCK_SIZE];
        ntfs_uncompblock(&mut out, &cb);
        assert_eq!(&out[..12], b"abababababab");
    }

    #[test]
    fn a_unit_is_decompressed_block_by_block() {
        let ntmp = Ntfsmount::new();
        ntmp.ntm_bootfile.set(Bootfile {
            bf_bps: 512,
            bf_spc: 2,
            ..Bootfile::default()
        });
        let unit = ntmp.ntfs_cntob(NTFS_COMPUNIT_CL) as usize;
        assert_eq!(unit, 16384);
        let src = sample(unit - 1000, 9);
        let mut cup = Vec::new();
        for (i, chunk) in src.chunks(NTFS_COMPBLOCK_SIZE).enumerate() {
            if i == 1 {
                let mut full = chunk.to_vec();
                full.resize(NTFS_COMPBLOCK_SIZE, 0);
                cup.extend_from_slice(&stored_block(&full));
            } else {
                cup.extend_from_slice(&compress_block(chunk));
            }
        }
        cup.resize(unit, 0);
        let mut uup = vec![0u8; unit];
        ntfs_uncompunit(&ntmp, &mut uup, &cup).unwrap();
        assert_eq!(&uup[..src.len()], &src[..]);
    }
}
/* </TESTS> */
