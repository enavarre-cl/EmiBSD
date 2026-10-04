/* <LICENSES> */
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
/* </LICENSES> */

//! `<ufs/ext2fs/ext2fs_extents.h>`: the ext4 extent tree on disk (`struct ext4_extent`,
//! `struct ext4_extent_index`, `struct ext4_extent_header`) and in core (the extent cache and
//! the path to an extent).
//!
//! Upstream: sys/ufs/ext2fs/ext2fs_extents.h @ 3ce1f3f79392
//!
//! This module is also the home of `ext2fs_extents.c` (`ext4_ext_in_cache`,
//! `ext4_ext_put_cache`, `ext4_ext_find_extent`), ported after the header by another entry
//! of `ports.toml`.
//!
//! The header has no `$OpenBSD$` line.
//!
//! ## Deviations
//! - The three on-disk structures are plain `#[repr(C)]` data (12 bytes each, pinned below),
//!   little-endian on disk.
//! - `struct ext4_extent_path` keeps its pointers into the tree node (`ep_ext`, `ep_index`,
//!   `ep_header`) as raw pointers: the root node lives in the inode's `e2di_blocks`, the
//!   others in the buffer `ep_bp`, so an offset into one buffer cannot name both. They are
//!   NULL until `ext4_ext_find_extent` sets them.
//! - The prototypes of the header are the functions of the `.c`'s part of this module.

use core::mem::size_of;
use core::ptr;

use crate::sys::buf::Buf;
use crate::sys::types::Daddr;

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
}

impl Default for Ext4ExtentPath {
    fn default() -> Self {
        Self::new()
    }
}

const _: () = {
    assert!(size_of::<Ext4Extent>() == 12);
    assert!(size_of::<Ext4ExtentIndex>() == 12);
    assert!(size_of::<Ext4ExtentHeader>() == 12);
};

#[cfg(test)]
mod tests {
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
}
