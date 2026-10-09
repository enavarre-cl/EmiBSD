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
/* </LICENSES> */

/* <CODE> */
//! The second extended file system: OpenBSD `sys/ufs/ext2fs/`, compiled with feature `ext2fs`
//! (`option EXT2FS`), on the UFS layer (`ufs/ufs`).
//!
//! Headers and leaves: `ext2fs` (`ext2fs.h`: the super block, the group descriptor and the
//! block arithmetic), `ext2fs_dinode` (the on-disk inode), `ext2fs_dir` (directory entries),
//! `ext2fs_extents` (the ext4 extent tree types; its `.c` is part of the same module),
//! `ext2fs_extern` (the shared declarations) and `ext2fs_bswap` (the byte swappers, used on
//! big-endian machines). The in-core super block of a mount is a [`ext2fs::MExt2fs`], reached
//! from an inode as `Inode::e2fs()` and from the mount as `Ufsmount::e2fs()`.
//!
//! The file system itself: `ext2fs_vfsops` (mounting, `vget`, `statfs`, `sync`),
//! `ext2fs_alloc` and `ext2fs_balloc` (blocks and inodes), `ext2fs_bmap` (logical to disk
//! blocks, by block pointers or extents), `ext2fs_inode` (size, update, truncate, inactive),
//! `ext2fs_readwrite` (`read`/`write`), `ext2fs_subr` (`bufatoff`, `vinit`),
//! `ext2fs_lookup` (directories: lookup, readdir and the entry routines) and `ext2fs_vnops`
//! (the vnode operations and their tables).

#[allow(clippy::module_inception)] // OpenBSD's sys/ufs/ext2fs/ext2fs.h
pub mod ext2fs;
pub mod ext2fs_alloc;
pub mod ext2fs_balloc;
pub mod ext2fs_bmap;
pub mod ext2fs_bswap;
pub mod ext2fs_dinode;
pub mod ext2fs_dir;
pub mod ext2fs_extents;
pub mod ext2fs_extern;
pub mod ext2fs_inode;
pub mod ext2fs_lookup;
pub mod ext2fs_readwrite;
pub mod ext2fs_subr;
pub mod ext2fs_vfsops;
pub mod ext2fs_vnops;
/* </CODE> */
