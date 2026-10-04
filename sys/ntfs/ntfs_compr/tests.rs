//! Host tests for LZNT1 decompression: a small compressor (greedy, the encoding of the C's
//! decoder: the split of a back reference's 16 bits between displacement and length widens
//! with the output position), stored and compressed blocks, back references that overlap
//! their output, and whole compression units.

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
