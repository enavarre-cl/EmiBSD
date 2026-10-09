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
 * Copyright (c) 2012, 2010 Zheng Liu <lz@freebsd.org>
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
 * $FreeBSD: head/sys/fs/ext2fs/ext2_extents.h 262623 2014-02-28 21:25:32Z pfg $
 */
/*-
 * Copyright (c) 2010 Zheng Liu <lz@freebsd.org>
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
 * $FreeBSD: head/sys/fs/ext2fs/ext2_extents.c 254260 2013-08-12 21:34:48Z pfg $
 */
/* </LICENSES> */

/* <CODE> */
//! `<ufs/ext2fs/ext2fs_extents.h>`: the ext4 extent tree on disk (`struct ext4_extent`,
//! `struct ext4_extent_index`, `struct ext4_extent_header`) and in core (the extent cache and
//! the path to an extent).
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_extents.h @ 3ce1f3f79392
//!
//! This module is also the home of `ext2fs_extents.c`: finding a block in the extent cache
//! (`ext4_ext_in_cache`), filling the cache (`ext4_ext_put_cache`) and walking the tree down
//! to the extent of a logical block (`ext4_ext_find_extent`, with the binary searches of an
//! index node and of a leaf).
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_extents.h @ 3ce1f3f79392
//! Upstream: sys/ufs/ext2fs/ext2fs_extents.c @ 3ce1f3f79392
//!
//! Neither file has an `$OpenBSD$` line; both keep their FreeBSD notices.
//!
//! ## Deviations
//! - The three on-disk structures are plain `#[repr(C)]` data (12 bytes each, pinned below),
//!   little-endian on disk.
//! - `struct ext4_extent_path` keeps its pointers into the tree node (`ep_ext`, `ep_index`,
//!   `ep_header`) as raw pointers: the root node lives in the inode's `e2di_blocks`, the
//!   others in the buffer `ep_bp`, so an offset into one buffer cannot name both. They are
//!   NULL until `ext4_ext_find_extent` sets them. The tree's entries are read with
//!   `read_unaligned`, as the C reads them in place.
//! - The prototypes of the header are the functions of the `.c`'s part of this module.
//! - `ext4_ext_find_extent` returns `Option<&mut Ext4ExtentPath>` (the C's path or NULL).
//!   Before searching a node it checks that the node's `eh_ecount` entries fit in it (the
//!   60 bytes of `e2di_blocks` for the root, the buffer for the others): a node that claims
//!   more is a corrupt tree, whose search the C would run past the node; here the search
//!   stops and the path comes back with `ep_ext` NULL (the callers' "no extent", `EIO`).
//!   The callers release `ep_bp` in every case.
//! - The binary searches keep the C's result for a block before the node's first entry: the
//!   entry "before the first", which is the node's header read as an entry (12 bytes as
//!   well, so inside the node).

use core::mem::size_of;
use core::ptr;

use crate::kern::subr_prf::panic;
use crate::kern::vfs_bio::{bread, brelse};
use crate::sys::buf::Buf;
use crate::sys::types::Daddr;
use crate::ufs::ext2fs::ext2fs::{MExt2fs, fsbtodb};
use crate::ufs::ext2fs::ext2fs_dinode::{Ext2fsDinode, NDADDR, NIADDR};
use crate::ufs::ufs::inode::Inode;

/// `EXT4_EXT_MAGIC`: the magic number of an extent tree header.
pub const EXT4_EXT_MAGIC: u16 = 0xf30a;

/// `EXT4_EXT_CACHE_NO`: nothing cached.
pub const EXT4_EXT_CACHE_NO: u32 = 0;
/// `EXT4_EXT_CACHE_GAP`: the cache holds a gap between extents.
pub const EXT4_EXT_CACHE_GAP: u32 = 1;
/// `EXT4_EXT_CACHE_IN`: the cache holds an extent.
pub const EXT4_EXT_CACHE_IN: u32 = 2;

/// `struct ext4_extent`: an ext4 file system extent on disk.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ext4Extent {
    /// `e_blk`: first logical block.
    pub e_blk: u32,
    /// `e_len`: number of blocks.
    pub e_len: u16,
    /// `e_start_hi`: high 16 bits of physical block.
    pub e_start_hi: u16,
    /// `e_start_lo`: low 32 bits of physical block.
    pub e_start_lo: u32,
}

/// `struct ext4_extent_index`: an extent index on disk.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ext4ExtentIndex {
    /// `ei_blk`: indexes logical blocks.
    pub ei_blk: u32,
    /// `ei_leaf_lo`: points to physical block of the next level.
    pub ei_leaf_lo: u32,
    /// `ei_leaf_hi`: high 16 bits of physical block.
    pub ei_leaf_hi: u16,
    /// `ei_unused`.
    pub ei_unused: u16,
}

/// `struct ext4_extent_header`: the extent tree header.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ext4ExtentHeader {
    /// `eh_magic`: magic number: `EXT4_EXT_MAGIC`.
    pub eh_magic: u16,
    /// `eh_ecount`: number of valid entries.
    pub eh_ecount: u16,
    /// `eh_max`: capacity of store in entries.
    pub eh_max: u16,
    /// `eh_depth`: the depth of the extent tree.
    pub eh_depth: u16,
    /// `eh_gen`: generation of the extent tree.
    pub eh_gen: u32,
}

/// `struct ext4_extent_cache`: the saved cached extent.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ext4ExtentCache {
    /// `ec_start`: extent start.
    pub ec_start: Daddr,
    /// `ec_blk`: logical block.
    pub ec_blk: u32,
    /// `ec_len`.
    pub ec_len: u32,
    /// `ec_type`: `EXT4_EXT_CACHE_*`.
    pub ec_type: u32,
}

/// `struct ext4_extent_path`: the saved path to some extent.
#[derive(Clone, Copy)]
pub struct Ext4ExtentPath {
    /// `ep_depth`.
    pub ep_depth: u16,
    /// `ep_bp`: the buffer holding this level's node (none for the root, in the inode).
    pub ep_bp: Option<&'static Buf>,
    /// `ep_ext`.
    pub ep_ext: *mut Ext4Extent,
    /// `ep_index`.
    pub ep_index: *mut Ext4ExtentIndex,
    /// `ep_header`.
    pub ep_header: *mut Ext4ExtentHeader,
}

impl Ext4ExtentPath {
    /// A path level with nothing found yet.
    pub const fn new() -> Self {
        Self {
            ep_depth: 0,
            ep_bp: None,
            ep_ext: ptr::null_mut(),
            ep_index: ptr::null_mut(),
            ep_header: ptr::null_mut(),
        }
    }

    /// `*path->ep_ext`: a copy of the extent the path found, `None` for the C's NULL. Read it
    /// before `ep_bp` is released: a leaf below the root lives in that buffer.
    pub fn ext(&self) -> Option<Ext4Extent> {
        if self.ep_ext.is_null() {
            return None;
        }
        // SAFETY: a non-NULL `ep_ext` is set by `ext4_ext_find_extent` to an entry of the
        // leaf (or its header, the entry before the first), inside the inode's block array
        // or the buffer `ep_bp` the path still holds; the read is unaligned.
        Some(unsafe { self.ep_ext.read_unaligned() })
    }
}

impl Default for Ext4ExtentPath {
    fn default() -> Self {
        Self::new()
    }
}

/// The bytes of a tree node's header and of each of its entries.
const EXT4_ENTRY_SIZE: usize = 12;

/// The number of entries of the node `path.ep_header` heads, or `None` when its header claims
/// more entries than the node holds (see the module's deviations). The root node is the
/// inode's `e2di_blocks`; the others fill the buffer `ep_bp`.
fn ext4_ext_node_entries(path: &Ext4ExtentPath) -> Option<usize> {
    let room = match path.ep_bp {
        Some(bp) => usize::try_from(bp.b_bcount.get()).unwrap_or(0),
        None => (NDADDR + NIADDR) * size_of::<u32>(),
    };
    // SAFETY: `ext4_ext_find_extent` points `ep_header` at the first bytes of the node, which
    // are inside the inode's dinode or the buffer it holds; the read is unaligned.
    let eh = unsafe { path.ep_header.read_unaligned() };
    let n = usize::from(eh.eh_ecount);
    (n <= room.saturating_sub(EXT4_ENTRY_SIZE) / EXT4_ENTRY_SIZE).then_some(n)
}

/// `ext4_ext_binsearch_index`: the index entry of node `path.ep_header` (with `n` entries)
/// whose subtree holds `lbn`, into `path.ep_index`.
fn ext4_ext_binsearch_index(_ip: &Inode, path: &mut Ext4ExtentPath, lbn: Daddr, n: usize) {
    let first = path.ep_header.wrapping_add(1).cast::<Ext4ExtentIndex>();
    let (mut l, mut r) = (0isize, n as isize - 1);
    while l <= r {
        let m = l + (r - l) / 2;
        // SAFETY: `0 <= m < n`, and the node holds `n` entries after its header
        // (`ext4_ext_node_entries`); the read is unaligned.
        let ei = unsafe { first.offset(m).read_unaligned() };
        if lbn < Daddr::from(ei.ei_blk) {
            r = m - 1;
        } else {
            l = m + 1;
        }
    }

    path.ep_index = first.wrapping_offset(l - 1);
}

/// `ext4_ext_binsearch`: the extent of leaf `path.ep_header` (with `n` entries) that may
/// hold `lbn`, into `path.ep_ext` (left as it is for an empty leaf).
fn ext4_ext_binsearch(_ip: &Inode, path: &mut Ext4ExtentPath, lbn: Daddr, n: usize) {
    if n == 0 {
        return;
    }

    let first = path.ep_header.wrapping_add(1).cast::<Ext4Extent>();
    let (mut l, mut r) = (0isize, n as isize - 1);
    while l <= r {
        let m = l + (r - l) / 2;
        // SAFETY: as in `ext4_ext_binsearch_index`.
        let e = unsafe { first.offset(m).read_unaligned() };
        if lbn < Daddr::from(e.e_blk) {
            r = m - 1;
        } else {
            l = m + 1;
        }
    }

    path.ep_ext = first.wrapping_offset(l - 1);
}

/// `ext4_ext_in_cache`: find a block in the ext4 extent cache. Fills `ep` and returns the
/// cache's `EXT4_EXT_CACHE_*` type when `lbn` is in the cached range, `EXT4_EXT_CACHE_NO`
/// otherwise.
pub fn ext4_ext_in_cache(ip: &Inode, lbn: Daddr, ep: &mut Ext4Extent) -> u32 {
    let ecp = ip.i_e2fs_ext_cache().get();
    let mut ret = EXT4_EXT_CACHE_NO;

    // cache is invalid
    if ecp.ec_type == EXT4_EXT_CACHE_NO {
        return ret;
    }

    if lbn >= Daddr::from(ecp.ec_blk) && lbn < Daddr::from(ecp.ec_blk.wrapping_add(ecp.ec_len)) {
        ep.e_blk = ecp.ec_blk;
        ep.e_start_lo = (ecp.ec_start & 0xffff_ffff) as u32;
        ep.e_start_hi = ((ecp.ec_start >> 32) & 0xffff) as u16;
        ep.e_len = ecp.ec_len as u16;
        ret = ecp.ec_type;
    }
    ret
}

/// `ext4_ext_put_cache`: put an extent in the ext4 cache, as `type_` (`EXT4_EXT_CACHE_*`).
pub fn ext4_ext_put_cache(ip: &Inode, ep: &Ext4Extent, type_: u32) {
    ip.i_e2fs_ext_cache().set(Ext4ExtentCache {
        ec_type: type_,
        ec_blk: ep.e_blk,
        ec_len: u32::from(ep.e_len),
        ec_start: (Daddr::from(ep.e_start_hi) << 32) | Daddr::from(ep.e_start_lo),
    });
}

/// `ext4_ext_find_extent`: find the extent of logical block `lbn` of `ip`. `None` when the
/// inode holds no extent tree or an index block cannot be read; otherwise the path, whose
/// `ep_ext` is the extent (NULL if there is none) and whose `ep_bp` holds the leaf's buffer
/// below the root, which the caller releases.
pub fn ext4_ext_find_extent<'a>(
    fs: &MExt2fs,
    ip: &Inode,
    lbn: Daddr,
    path: &'a mut Ext4ExtentPath,
) -> Option<&'a mut Ext4ExtentPath> {
    let din = ip.dinode_u.get().cast::<Ext2fsDinode>();
    if din.is_null() {
        panic(format_args!(
            "ext4_ext_find_extent: inode {:p} has no dinode",
            ip
        ));
    }
    // SAFETY: `dinode_u` is the inode's own pool dinode (`Inode::with_e2din`'s invariant);
    // the place is named without making a reference.
    let mut ehp = unsafe { &raw mut (*din).e2di_blocks }.cast::<Ext4ExtentHeader>();

    // SAFETY: the block array holds 60 bytes, a header's worth and more; unaligned read.
    let eh = unsafe { ehp.read_unaligned() };
    if eh.eh_magic != EXT4_EXT_MAGIC {
        return None;
    }

    path.ep_header = ehp;

    let mut i = eh.eh_depth;
    while i != 0 {
        let Some(n) = ext4_ext_node_entries(path) else {
            path.ep_ext = ptr::null_mut();
            return Some(path);
        };
        ext4_ext_binsearch_index(ip, path, lbn, n);
        path.ep_depth = 0;
        path.ep_ext = ptr::null_mut();

        // SAFETY: `ep_index` is one of the node's entries or its header (the entry before
        // the first), 12 bytes inside the node either way; unaligned read.
        let ei = unsafe { path.ep_index.read_unaligned() };
        let nblk = (Daddr::from(ei.ei_leaf_hi) << 32) | Daddr::from(ei.ei_leaf_lo);
        if let Some(bp) = path.ep_bp.take() {
            brelse(bp);
        }
        let (bp, error) = bread(ip.i_devvp(), fsbtodb(fs, nblk), fs.e2fs_fsize.get());
        if error.is_err() {
            brelse(bp);
            path.ep_bp = None;
            return None;
        }
        path.ep_bp = Some(bp);
        ehp = bp.b_data.get().cast::<Ext4ExtentHeader>();
        path.ep_header = ehp;
        i -= 1;
    }

    path.ep_depth = i;
    path.ep_ext = ptr::null_mut();
    path.ep_index = ptr::null_mut();

    if let Some(n) = ext4_ext_node_entries(path) {
        ext4_ext_binsearch(ip, path, lbn, n);
    }
    Some(path)
}

const _: () = {
    assert!(size_of::<Ext4Extent>() == 12);
    assert!(size_of::<Ext4ExtentIndex>() == 12);
    assert!(size_of::<Ext4ExtentHeader>() == 12);
};
/* </CODE> */

/* <TESTS> */
#[cfg(test)]
mod tests {
    // Host tests for the extent tree: the constants against the C header, `ext4_ext_find_extent`
    // over a root node held in an in-memory inode (the deeper levels, read through the buffer
    // cache, are exercised by the mount tests of `ext2fs_vfsops`), and the extent cache.

    use std::boxed::Box;
    use std::{assert, assert_eq};

    use super::*;

    #[test]
    #[ignore = "needs OPENBSD_SRC (just test-ref)"]
    fn constants_match_the_c_header() {
        let defs = crate::reftest::defines("sys/ufs/ext2fs/ext2fs_extents.h");
        for (name, value) in [
            ("EXT4_EXT_MAGIC", i64::from(EXT4_EXT_MAGIC)),
            ("EXT4_EXT_CACHE_NO", i64::from(EXT4_EXT_CACHE_NO)),
            ("EXT4_EXT_CACHE_GAP", i64::from(EXT4_EXT_CACHE_GAP)),
            ("EXT4_EXT_CACHE_IN", i64::from(EXT4_EXT_CACHE_IN)),
        ] {
            assert_eq!(crate::reftest::int(&defs, name), Some(value), "{name}");
        }
    }

    /// An inode whose block array is an extent tree root: `ecount` entries of `extents`
    /// (`(first logical block, length, first disk block)`), with `magic`.
    fn inode(magic: u16, ecount: u16, extents: &[(u32, u16, u32)]) -> &'static Inode {
        let mut blocks = [0u32; NDADDR + NIADDR];
        blocks[0] = u32::from(magic) | u32::from(ecount) << 16;
        blocks[1] = 4; // eh_max, eh_depth 0
        for (i, &(blk, len, start)) in extents.iter().enumerate() {
            blocks[3 + 3 * i] = blk;
            blocks[4 + 3 * i] = u32::from(len);
            blocks[5 + 3 * i] = start;
        }
        let din: &'static mut Ext2fsDinode = Box::leak(Box::new(Ext2fsDinode {
            e2di_blocks: blocks,
            ..Ext2fsDinode::default()
        }));
        let ip: &'static Inode = Box::leak(Box::new(Inode::new()));
        ip.dinode_u.set(ptr::from_mut(din).cast());
        ip
    }

    /// `ext4_ext_find_extent` at depth 0: the extent found, if any.
    fn find(ip: &Inode, lbn: Daddr) -> Option<Option<Ext4Extent>> {
        let fs = MExt2fs::new();
        let mut path = Ext4ExtentPath::new();
        ext4_ext_find_extent(&fs, ip, lbn, &mut path).map(|p| {
            assert!(p.ep_bp.is_none());
            assert_eq!(p.ep_depth, 0);
            p.ext()
        })
    }

    #[test]
    fn the_root_leaf_is_searched_for_the_extent_at_or_before_a_block() {
        let e = |blk, len, start| Ext4Extent {
            e_blk: blk,
            e_len: len,
            e_start_hi: 0,
            e_start_lo: start,
        };
        let ip = inode(
            EXT4_EXT_MAGIC,
            3,
            &[(5, 4, 100), (10, 2, 200), (20, 1, 300)],
        );
        assert_eq!(find(ip, 5), Some(Some(e(5, 4, 100))));
        assert_eq!(find(ip, 8), Some(Some(e(5, 4, 100))));
        assert_eq!(find(ip, 10), Some(Some(e(10, 2, 200))));
        // Past an extent's end the C still answers with it; the callers do the arithmetic.
        assert_eq!(find(ip, 15), Some(Some(e(10, 2, 200))));
        assert_eq!(find(ip, 1000), Some(Some(e(20, 1, 300))));

        // Before the first extent: the entry before it, the header read as an extent.
        let fs = MExt2fs::new();
        let mut path = Ext4ExtentPath::new();
        ext4_ext_find_extent(&fs, ip, 2, &mut path);
        assert_eq!(path.ep_ext.cast::<Ext4ExtentHeader>(), path.ep_header);

        // An empty leaf finds nothing; no tree at all is no path.
        assert_eq!(find(inode(EXT4_EXT_MAGIC, 0, &[]), 3), Some(None));
        assert_eq!(find(inode(0xef53, 1, &[(0, 1, 9)]), 0), None);
        // A header claiming more entries than the root holds (4) is a corrupt tree.
        assert_eq!(find(inode(EXT4_EXT_MAGIC, 5, &[(0, 1, 9)]), 0), Some(None));
        assert_eq!(
            find(inode(EXT4_EXT_MAGIC, 4, &[(0, 1, 9)]), 0).map(|e| e.is_some()),
            Some(true)
        );
    }

    #[test]
    fn the_extent_cache_answers_inside_its_range_only() {
        let ip = inode(EXT4_EXT_MAGIC, 0, &[]);
        let mut ep = Ext4Extent::default();
        assert_eq!(ext4_ext_in_cache(ip, 0, &mut ep), EXT4_EXT_CACHE_NO);

        let cached = Ext4Extent {
            e_blk: 10,
            e_len: 2,
            e_start_hi: 1,
            e_start_lo: 5,
        };
        ext4_ext_put_cache(ip, &cached, EXT4_EXT_CACHE_IN);
        assert_eq!(ip.i_e2fs_ext_cache().get().ec_start, (1 << 32) | 5);
        assert_eq!(ext4_ext_in_cache(ip, 11, &mut ep), EXT4_EXT_CACHE_IN);
        assert_eq!(ep, cached);
        let mut other = Ext4Extent::default();
        assert_eq!(ext4_ext_in_cache(ip, 12, &mut other), EXT4_EXT_CACHE_NO);
        assert_eq!(ext4_ext_in_cache(ip, 9, &mut other), EXT4_EXT_CACHE_NO);
        assert_eq!(other, Ext4Extent::default());

        ext4_ext_put_cache(ip, &cached, EXT4_EXT_CACHE_GAP);
        assert_eq!(ext4_ext_in_cache(ip, 10, &mut ep), EXT4_EXT_CACHE_GAP);
    }
}
/* </TESTS> */
